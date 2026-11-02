// Messages between the main thread and the "trombone" AudioWorklet.

export type Setting = 'tractLength' | 'jitter' | 'nasality' | 'rate' | 'expressiveness';

export type ToWorklet =
	| { type: 'noteOn'; note: number }
	| { type: 'noteOff' }
	| { type: 'note'; note: number }
	| { type: 'tongue'; index: number; diameter: number }
	| { type: 'touch'; touch: number; index: number; diameter: number }
	| { type: 'release'; touch: number }
	| { type: 'articulate'; symbol: string }
	| { type: 'velum'; value: number }
	| { type: 'tenseness'; value: number }
	| { type: 'vibrato'; value: number }
	| { type: 'wobble'; on: boolean }
	| { type: 'drone'; on: boolean }
	| { type: 'reverb'; value: number }
	| { type: 'preset'; name: string }
	| { type: 'setting'; key: Setting; value: number }
	| { type: 'speak'; text: string; pitch: number }
	| { type: 'stop' };

/** `[pitch, tractLength, tenseness, expressiveness, rate, vibrato, jitter, nasality]` */
export type SettingsTuple = number[];

export type FromWorklet =
	| {
			type: 'ready';
			presets: string[];
			landmarks: number[];
			cells: number;
			noseCells: number;
			settings: SettingsTuple;
	  }
	| {
			type: 'frame';
			diameters: Float32Array;
			targets: Float32Array;
			amplitudes: Float32Array;
			nose: Float32Array;
			noseAmplitudes: Float32Array;
			/** `[intensity, frequency, velum, tenseness, tongueIndex, tongueDiameter, tractLength]` */
			status: number[];
			speaking: number;
	  }
	| { type: 'preset'; name: string; note: number; settings: SettingsTuple }
	| { type: 'utterance'; symbols: string[]; durations: number[] }
	| { type: 'error'; message: string };
