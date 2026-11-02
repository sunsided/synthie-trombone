// AudioWorklet: runs the wasm engine and streams the tract state back to the
// page about 60 times a second for drawing.
import './text-polyfill.js';
import { initSync, Engine } from '../wasm/pkg/engine.js';
import type { FromWorklet, ToWorklet } from './protocol';

// AudioWorkletGlobalScope globals.
declare const sampleRate: number;
declare class AudioWorkletProcessor {
	readonly port: MessagePort;
}
declare function registerProcessor(name: string, ctor: unknown): void;

/** Render quanta between frames sent to the page (~16 ms at 48 kHz). */
const QUANTA_PER_FRAME = 6;

class TromboneProcessor extends AudioWorkletProcessor {
	engine: Engine;
	mono = new Float32Array(128);
	quanta = 0;
	cells: number;
	noseCells: number;

	constructor(options: { processorOptions: { module: WebAssembly.Module } }) {
		super();
		// Field initializers run before this body; nothing may touch wasm
		// until initSync has run.
		initSync({ module: options.processorOptions.module });
		this.engine = new Engine(sampleRate);
		this.cells = Engine.cells();
		this.noseCells = Engine.noseCells();
		this.port.onmessage = ({ data }: MessageEvent<ToWorklet>) => this.handle(data);
		this.post({
			type: 'ready',
			presets: Engine.presetNames(),
			landmarks: Array.from(Engine.landmarks()),
			cells: this.cells,
			noseCells: this.noseCells,
			settings: Array.from(this.engine.settings())
		});
	}

	post(msg: FromWorklet, transfer: Transferable[] = []) {
		this.port.postMessage(msg, transfer);
	}

	handle(msg: ToWorklet) {
		const e = this.engine;
		try {
			switch (msg.type) {
				case 'noteOn': e.noteOn(msg.note); break;
				case 'noteOff': e.noteOff(); break;
				case 'note': e.setNote(msg.note); break;
				case 'tongue': e.setTongue(msg.index, msg.diameter); break;
				case 'touch': e.setTouch(msg.touch, msg.index, msg.diameter); break;
				case 'release': e.releaseTouch(msg.touch); break;
				case 'articulate': e.articulate(msg.symbol); break;
				case 'velum': e.setVelum(msg.value); break;
				case 'tenseness': e.setTenseness(msg.value); break;
				case 'vibrato': e.setVibrato(msg.value); break;
				case 'wobble': e.setWobble(msg.on); break;
				case 'drone': e.setDrone(msg.on); break;
				case 'reverb': e.setReverb(msg.value); break;
				case 'preset': {
					const note = e.setPreset(msg.name);
					if (note !== undefined) {
						this.post({ type: 'preset', name: msg.name, note, settings: Array.from(e.settings()) });
					}
					break;
				}
				case 'setting':
					switch (msg.key) {
						case 'tractLength': e.setTractLength(msg.value); break;
						case 'jitter': e.setJitter(msg.value); break;
						case 'nasality': e.setNasality(msg.value); break;
						case 'rate': e.setRate(msg.value); break;
						case 'expressiveness': e.setExpressiveness(msg.value); break;
					}
					break;
				case 'speak':
					e.speak(msg.text, msg.pitch);
					this.post({
						type: 'utterance',
						symbols: e.symbols(),
						durations: Array.from(e.durations())
					});
					break;
				case 'stop': e.stopSpeaking(); e.noteOff(); break;
			}
		} catch (err) {
			this.post({ type: 'error', message: String((err as Error)?.message ?? err) });
		}
	}

	process(_inputs: Float32Array[][], outputs: Float32Array[][]) {
		const output = outputs[0];
		const frames = output[0].length;
		if (this.mono.length !== frames) this.mono = new Float32Array(frames);
		this.engine.render(this.mono);
		for (const channel of output) channel.set(this.mono);

		if (++this.quanta >= QUANTA_PER_FRAME) {
			this.quanta = 0;
			this.sendFrame();
		}
		return true;
	}

	sendFrame() {
		const e = this.engine;
		const diameters = new Float32Array(this.cells);
		const targets = new Float32Array(this.cells);
		const amplitudes = new Float32Array(this.cells);
		const nose = new Float32Array(this.noseCells);
		const noseAmplitudes = new Float32Array(this.noseCells);
		e.fillDiameters(diameters);
		e.fillTargets(targets);
		e.fillAmplitudes(amplitudes);
		e.fillNose(nose);
		e.fillNoseAmplitudes(noseAmplitudes);
		this.post(
			{
				type: 'frame',
				diameters,
				targets,
				amplitudes,
				nose,
				noseAmplitudes,
				status: Array.from(e.status()),
				speaking: e.speakingIndex()
			},
			[diameters.buffer, targets.buffer, amplitudes.buffer, nose.buffer, noseAmplitudes.buffer]
		);
	}
}

registerProcessor('trombone', TromboneProcessor);
