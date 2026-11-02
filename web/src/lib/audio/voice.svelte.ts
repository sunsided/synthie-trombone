// Main-thread side of the "trombone" AudioWorklet: starts audio on a user
// gesture, sends commands, and keeps the latest tract state for drawing.
import wasmUrl from '../wasm/pkg/engine_bg.wasm?url';
import workletUrl from './worklet.ts?worker&url';
import type { FromWorklet, Setting, SettingsTuple, ToWorklet } from './protocol';

/** Tract state from the last worklet frame; read by the canvas every frame. */
export interface TractFrame {
	diameters: Float32Array;
	targets: Float32Array;
	amplitudes: Float32Array;
	nose: Float32Array;
	noseAmplitudes: Float32Array;
}

export interface VoiceSettings {
	pitch: number;
	tractLength: number;
	tenseness: number;
	expressiveness: number;
	rate: number;
	vibrato: number;
	jitter: number;
	nasality: number;
}

function toSettings(t: SettingsTuple): VoiceSettings {
	const [pitch, tractLength, tenseness, expressiveness, rate, vibrato, jitter, nasality] = t;
	return { pitch, tractLength, tenseness, expressiveness, rate, vibrato, jitter, nasality };
}

export class Voice {
	/** Audio is running. */
	awake = $state(false);
	starting = $state(false);
	error = $state<string | null>(null);

	presets = $state<string[]>([]);
	preset = $state('deep');
	settings = $state<VoiceSettings | null>(null);
	/** MIDI note the voicebox and speech start from. */
	baseNote = $state(45);

	intensity = $state(0);
	frequency = $state(0);
	velum = $state(0);
	tongue = $state({ index: 12.9, diameter: 2.43 });

	symbols = $state<string[]>([]);
	durations = $state<number[]>([]);
	speaking = $state(-1);

	/** Cell indices: nose start, blade start, tip start, lip start. */
	landmarks = { noseStart: 17, bladeStart: 10, tipStart: 32, lipStart: 39 };
	cells = 44;
	noseCells = 28;
	frame: TractFrame | null = null;
	analyser: AnalyserNode | null = null;

	#ctx: AudioContext | null = null;
	#node: AudioWorkletNode | null = null;

	async start() {
		if (this.awake || this.starting) return;
		this.starting = true;
		this.error = null;
		try {
			const ctx = new AudioContext({ latencyHint: 'interactive' });
			const [module] = await Promise.all([
				WebAssembly.compileStreaming(fetch(wasmUrl)),
				ctx.audioWorklet.addModule(workletUrl)
			]);
			const node = new AudioWorkletNode(ctx, 'trombone', {
				outputChannelCount: [2],
				processorOptions: { module }
			});
			const ready = new Promise<void>((resolve, reject) => {
				const timeout = setTimeout(() => reject(new Error('the audio worklet did not answer')), 8000);
				node.onprocessorerror = () => {
					clearTimeout(timeout);
					reject(new Error('the audio worklet failed to start'));
				};
				node.port.onmessage = ({ data }: MessageEvent<FromWorklet>) => {
					this.#receive(data);
					if (data.type === 'ready') {
						clearTimeout(timeout);
						resolve();
					}
				};
			});
			const analyser = ctx.createAnalyser();
			analyser.fftSize = 2048;
			analyser.smoothingTimeConstant = 0.6;
			node.connect(analyser).connect(ctx.destination);
			await ready;
			await ctx.resume();
			this.#ctx = ctx;
			this.#node = node;
			this.analyser = analyser;
			this.awake = true;
		} catch (e) {
			this.error = `Audio could not start: ${(e as Error)?.message ?? e}`;
		} finally {
			this.starting = false;
		}
	}

	#receive(msg: FromWorklet) {
		switch (msg.type) {
			case 'ready': {
				const [noseStart, bladeStart, tipStart, lipStart] = msg.landmarks;
				this.landmarks = { noseStart, bladeStart, tipStart, lipStart };
				this.cells = msg.cells;
				this.noseCells = msg.noseCells;
				this.presets = msg.presets;
				this.settings = toSettings(msg.settings);
				break;
			}
			case 'frame': {
				this.frame = msg;
				const [intensity, frequency, velum, , ti, td] = msg.status;
				this.intensity = intensity;
				this.frequency = frequency;
				this.velum = velum;
				if (ti !== this.tongue.index || td !== this.tongue.diameter) {
					this.tongue = { index: ti, diameter: td };
				}
				this.speaking = msg.speaking;
				break;
			}
			case 'preset':
				this.preset = msg.name;
				this.settings = toSettings(msg.settings);
				this.baseNote = Math.round(msg.note);
				break;
			case 'utterance':
				this.symbols = msg.symbols;
				this.durations = msg.durations;
				break;
			case 'error':
				this.error = msg.message;
				break;
		}
	}

	send(msg: ToWorklet) {
		this.#node?.port.postMessage(msg);
	}

	noteOn(note: number) {
		this.send({ type: 'noteOn', note });
	}
	noteOff() {
		this.send({ type: 'noteOff' });
	}
	setNote(note: number) {
		this.send({ type: 'note', note });
	}
	setTongue(index: number, diameter: number) {
		this.tongue = { index, diameter };
		this.send({ type: 'tongue', index, diameter });
	}
	touch(touch: number, index: number, diameter: number) {
		this.send({ type: 'touch', touch, index, diameter });
	}
	release(touch: number) {
		this.send({ type: 'release', touch });
	}
	articulate(symbol: string) {
		this.send({ type: 'articulate', symbol });
	}
	setPreset(name: string) {
		this.send({ type: 'preset', name });
	}
	setSetting(key: Setting, value: number) {
		if (this.settings) this.settings[key] = value;
		this.send({ type: 'setting', key, value });
	}
	setTenseness(value: number) {
		if (this.settings) this.settings.tenseness = value;
		this.send({ type: 'tenseness', value });
	}
	setVibrato(value: number) {
		if (this.settings) this.settings.vibrato = value;
		this.send({ type: 'vibrato', value });
	}
	speak(text: string) {
		const pitch = 440 * 2 ** ((this.baseNote - 69) / 12);
		this.send({ type: 'speak', text, pitch });
	}
	stop() {
		this.send({ type: 'stop' });
	}
}
