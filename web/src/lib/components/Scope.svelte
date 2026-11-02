<script lang="ts">
	import type { Voice } from '#lib/audio/voice.svelte.ts';

	interface Props {
		voice: Voice;
	}

	let { voice }: Props = $props();

	let canvas: HTMLCanvasElement;

	$effect(() => {
		const ctx = canvas.getContext('2d');
		if (!ctx) return;
		let raf = 0;
		let samples = new Float32Array(2048);
		let spectrum = new Uint8Array(1024);

		const frame = () => {
			const dpr = window.devicePixelRatio || 1;
			const { width, height } = canvas.getBoundingClientRect();
			if (canvas.width !== Math.round(width * dpr)) canvas.width = Math.round(width * dpr);
			if (canvas.height !== Math.round(height * dpr)) canvas.height = Math.round(height * dpr);
			ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
			ctx.clearRect(0, 0, width, height);

			const analyser = voice.analyser;
			if (analyser) {
				if (samples.length !== analyser.fftSize) samples = new Float32Array(analyser.fftSize);
				if (spectrum.length !== analyser.frequencyBinCount)
					spectrum = new Uint8Array(analyser.frequencyBinCount);
				analyser.getFloatTimeDomainData(samples);
				analyser.getByteFrequencyData(spectrum);

				// Spectrum up to 5 kHz: the formants, as soft pink hills.
				const nyquist = analyser.context.sampleRate / 2;
				const bins = Math.floor((5000 / nyquist) * spectrum.length);
				ctx.beginPath();
				ctx.moveTo(0, height);
				for (let i = 0; i < bins; i++) {
					ctx.lineTo((i / (bins - 1)) * width, height - (spectrum[i] / 255) * height * 0.9);
				}
				ctx.lineTo(width, height);
				ctx.closePath();
				ctx.fillStyle = 'rgba(255, 79, 154, 0.22)';
				ctx.fill();

				// Waveform, triggered on a rising zero crossing so it stands still.
				let start = 0;
				for (let i = 1; i < samples.length / 2; i++) {
					if (samples[i - 1] < 0 && samples[i] >= 0) {
						start = i;
						break;
					}
				}
				const show = Math.floor(samples.length / 2);
				let peak = 0.05;
				for (let i = start; i < start + show; i++) peak = Math.max(peak, Math.abs(samples[i]));
				ctx.beginPath();
				for (let i = 0; i < show; i++) {
					const x = (i / (show - 1)) * width;
					const y = height / 2 - (samples[start + i] / peak) * height * 0.38;
					if (i === 0) ctx.moveTo(x, y);
					else ctx.lineTo(x, y);
				}
				ctx.lineWidth = 2;
				ctx.strokeStyle = '#5ff2ff';
				ctx.shadowColor = '#5ff2ff';
				ctx.shadowBlur = 8;
				ctx.stroke();
				ctx.shadowBlur = 0;
			}
			raf = requestAnimationFrame(frame);
		};
		raf = requestAnimationFrame(frame);
		return () => cancelAnimationFrame(raf);
	});
</script>

<canvas bind:this={canvas} aria-hidden="true"></canvas>

<style>
	canvas {
		display: block;
		width: 100%;
		height: 100%;
	}
</style>
