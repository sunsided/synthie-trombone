<script lang="ts">
	import type { Voice } from '#lib/audio/voice.svelte.ts';
	import {
		NOSE_OFFSET,
		TONGUE,
		VIEW,
		locate,
		onTongue,
		point,
		type Point
	} from '#lib/geometry.ts';

	interface Props {
		voice: Voice;
	}

	let { voice }: Props = $props();

	let canvas: HTMLCanvasElement;
	/** Active pointers: the tongue, or a finger in touch slot 0/1. */
	const grabs = new Map<number, { kind: 'tongue' } | { kind: 'touch'; slot: number; at: Point }>();

	/** Vowel markers inside the tongue region: IPA label, ARPAbet, (index, diameter). */
	const VOWELS: [string, string, number, number][] = [
		['i', 'IY', 28.0, 2.15],
		['ɪ', 'IH', 29.0, 2.8],
		['ɛ', 'EH', 28.0, 3.5],
		['æ', 'AE', 22.0, 2.95],
		['ʌ', 'AH', 18.0, 2.15],
		['ɑ', 'AA', 16.5, 2.25],
		['u', 'UW', 23.0, 2.05]
	];

	const COLORS = {
		ground: '#1a0d1f',
		mucosa: '#ff4f9a',
		breath: '#5ff2ff',
		brass: '#ffc94d',
		bone: '#f3e9f7',
		cartilage: '#8c6aa6'
	};

	// Map between CSS pixels and the original's drawing units.
	let view = { scale: 1, ox: 0, oy: 0 };

	function toTract(e: PointerEvent): Point {
		const rect = canvas.getBoundingClientRect();
		return {
			x: (e.clientX - rect.left - view.ox) / view.scale + VIEW.x,
			y: (e.clientY - rect.top - view.oy) / view.scale + VIEW.y
		};
	}

	function freeSlot(): number | undefined {
		const used = new Set(
			[...grabs.values()].flatMap((g) => (g.kind === 'touch' ? [g.slot] : []))
		);
		return [0, 1].find((s) => !used.has(s));
	}

	function apply(id: number, p: Point) {
		const grab = grabs.get(id);
		if (!grab) return;
		const { index, diameter } = locate(p, voice.landmarks.lipStart);
		if (grab.kind === 'tongue') {
			voice.setTongue(
				Math.min(TONGUE.maxIndex, Math.max(TONGUE.minIndex, index)),
				Math.min(TONGUE.maxDiameter, Math.max(TONGUE.minDiameter, diameter))
			);
		} else {
			grab.at = p;
			voice.touch(grab.slot, index, diameter);
		}
	}

	function down(e: PointerEvent) {
		if (!voice.awake) return;
		const p = toTract(e);
		const { index, diameter } = locate(p, voice.landmarks.lipStart);
		if (index < 0 || index > voice.cells + 2 || diameter < -1.2 || diameter > 4.2) return;
		if (onTongue(index, diameter) && ![...grabs.values()].some((g) => g.kind === 'tongue')) {
			grabs.set(e.pointerId, { kind: 'tongue' });
		} else {
			const slot = freeSlot();
			if (slot === undefined) return;
			grabs.set(e.pointerId, { kind: 'touch', slot, at: p });
		}
		canvas.setPointerCapture(e.pointerId);
		apply(e.pointerId, p);
	}

	function move(e: PointerEvent) {
		if (grabs.has(e.pointerId)) apply(e.pointerId, toTract(e));
	}

	function up(e: PointerEvent) {
		const grab = grabs.get(e.pointerId);
		if (!grab) return;
		grabs.delete(e.pointerId);
		if (grab.kind === 'touch') voice.release(grab.slot);
	}

	// ----- drawing ----------------------------------------------------------

	function lerp(d: Float32Array, x: number): number {
		const i = Math.max(0, Math.min(d.length - 1, Math.floor(x)));
		const j = Math.min(d.length - 1, i + 1);
		const t = x - i;
		return d[i] * (1 - t) + d[j] * t;
	}

	function curve(ctx: CanvasRenderingContext2D, from: number, to: number, d: (i: number) => number, step = 0.25, move = true) {
		const lip = voice.landmarks.lipStart;
		// `step` is negative when walking back from the lips to the glottis.
		const sign = Math.sign(step);
		for (let i = from; sign * (to - i) >= -1e-6; i += step) {
			const p = point(i, d(i), lip);
			if (move && i === from) ctx.moveTo(p.x, p.y);
			else ctx.lineTo(p.x, p.y);
		}
	}

	/** synthie's signature gradient: cyan → purple → magenta → orange. */
	const RAINBOW: [number, number, number][] = [
		[0, 240, 255],
		[168, 92, 255],
		[255, 46, 170],
		[255, 140, 40]
	];

	function rainbow(t: number): string {
		const pos = Math.min(1, Math.max(0, t)) * (RAINBOW.length - 1);
		const i = Math.min(RAINBOW.length - 2, Math.floor(pos));
		const f = pos - i;
		const [a, b] = [RAINBOW[i], RAINBOW[i + 1]];
		const c = a.map((x, k) => Math.round(x + (b[k] - x) * f));
		return `rgb(${c[0]}, ${c[1]}, ${c[2]})`;
	}

	/** Ping-pong through the gradient so it can scroll without a seam, like the TUI wordmark. */
	function rainbowCyclic(t: number): string {
		const f = ((t % 1) + 1) % 1;
		return rainbow(1 - Math.abs(2 * f - 1));
	}

	/** Animation time at which sound came on. */
	let wokeAt: number | null = null;

	const reducedMotion =
		typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;

	function textOnArc(
		ctx: CanvasRenderingContext2D,
		text: string,
		startIndex: number,
		diameter: number,
		spacing: number,
		color?: (k: number, count: number) => string
	) {
		const lip = voice.landmarks.lipStart;
		const count = [...text].length;
		let index = startIndex;
		let k = 0;
		for (const ch of text) {
			if (color) ctx.fillStyle = color(k++, count);
			const w = ctx.measureText(ch).width;
			// Advance along the arc by the glyph's width at this radius.
			const r = 298 - 60 * diameter;
			const step = ((w + spacing) / r) * ((lip - 1) / (0.64 * Math.PI));
			const mid = index + step / 2;
			const p = point(mid, diameter, lip);
			const a = -0.24 + (mid * 0.64 * Math.PI) / (lip - 1);
			ctx.save();
			ctx.translate(p.x, p.y);
			ctx.rotate(a - Math.PI / 2);
			ctx.fillText(ch, -w / 2, 0);
			ctx.restore();
			index += step;
		}
	}

	function draw(ctx: CanvasRenderingContext2D, t: number) {
		const f = voice.frame;
		const { noseStart, lipStart } = voice.landmarks;
		const n = voice.cells;
		const lip = lipStart;
		const dia = (i: number) => (f ? lerp(f.diameters, i) : 1.5);
		const amp = (i: number) => (f ? lerp(f.amplitudes, i) : 0);
		const breath = voice.awake ? Math.min(1, voice.intensity * 1.2) : 0;

		// Title along the outside of the arc, in synthie's scrolling rainbow.
		ctx.font = "800 54px 'Bricolage Grotesque Variable', sans-serif";
		ctx.textBaseline = 'alphabetic';
		const shift = reducedMotion ? 0 : (t / 1000) * 0.15;
		textOnArc(ctx, 'synthie trombone', 3.2, -3.25, 3, (k, count) =>
			rainbowCyclic((k / count) * 0.5 + shift)
		);

		// Everything below the title is dimmed until sound is on, then fades
		// up over 1.4 s: the throat drawing breath.
		if (voice.awake && wokeAt === null) wokeAt = t;
		const rise = wokeAt === null ? 0 : reducedMotion ? 1 : Math.min(1, (t - wokeAt) / 1400);
		ctx.globalAlpha = 0.45 + 0.55 * rise * (2 - rise);

		// Tissue: the tongue and jaw as bands that hug the floor of the
		// airway and fade inwards, the palate as a thin band above.
		// Strokes offset inwards follow the curvature cleanly where filled
		// polygons would fold over themselves.
		const bands: [number, number, string][] = [
			[0.75, 0.7, 'rgba(255, 79, 154, 0.07)'],
			[0.32, 0.64, 'rgba(255, 79, 154, 0.18)']
		];
		ctx.lineJoin = 'round';
		ctx.lineCap = 'round';
		for (const [offset, width, color] of bands) {
			ctx.beginPath();
			curve(ctx, 0.3, n - 1.2, (i) => dia(i) + offset);
			ctx.lineWidth = width * 60;
			ctx.strokeStyle = color;
			ctx.stroke();
		}

		ctx.beginPath();
		curve(ctx, 0, n - 1, () => 0);
		curve(ctx, n - 1, 0, () => -0.45, -0.5, false);
		ctx.closePath();
		ctx.fillStyle = 'rgba(255, 79, 154, 0.38)';
		ctx.fill();

		// Airway, lit by the pressure in each cell.
		for (let i = 0; i < n - 1; i += 0.5) {
			const a = Math.min(1, amp(i) * 1.8);
			ctx.beginPath();
			const p0 = point(i, 0, lip);
			const p1 = point(i + 0.5, 0, lip);
			const p2 = point(i + 0.5, dia(i + 0.5), lip);
			const p3 = point(i, dia(i), lip);
			ctx.moveTo(p0.x, p0.y);
			ctx.lineTo(p1.x, p1.y);
			ctx.lineTo(p2.x, p2.y);
			ctx.lineTo(p3.x, p3.y);
			ctx.closePath();
			ctx.fillStyle = `rgba(95, 242, 255, ${0.04 + 0.5 * a * breath})`;
			ctx.fill();
		}

		// Nasal cavity, above the palate; the velum opens the passage at noseStart.
		if (f) {
			const nose = (k: number) => lerp(f.nose, k);
			ctx.beginPath();
			curve(ctx, noseStart, n - 1, () => -NOSE_OFFSET);
			curve(ctx, n - 1, noseStart, (i) => -NOSE_OFFSET - nose(i - noseStart) * 0.9, -0.25, false);
			ctx.closePath();
			const noseLight = Math.min(1, lerp(f.noseAmplitudes, 10) * 2.5) * breath;
			ctx.fillStyle = `rgba(95, 242, 255, ${0.05 + 0.45 * noseLight})`;
			ctx.fill();
			ctx.lineWidth = 2.5;
			ctx.strokeStyle = COLORS.mucosa;
			ctx.shadowColor = COLORS.mucosa;
			ctx.shadowBlur = 12;
			ctx.stroke();

			// Velum passage, drawn only while it is open.
			const open = voice.velum;
			if (open > 0.04) {
			const v0 = point(noseStart - 0.5 - open * 1.5, 0, lip);
			const v1 = point(noseStart - 0.5 - open * 1.5, -NOSE_OFFSET, lip);
			const v2 = point(noseStart + 0.5 + open * 1.5, -NOSE_OFFSET, lip);
			const v3 = point(noseStart + 0.5 + open * 1.5, 0, lip);
			ctx.beginPath();
			ctx.moveTo(v0.x, v0.y);
			ctx.lineTo(v1.x, v1.y);
			ctx.lineTo(v2.x, v2.y);
			ctx.lineTo(v3.x, v3.y);
			ctx.closePath();
			ctx.fillStyle = `rgba(95, 242, 255, ${0.15 + 0.5 * open * breath})`;
			ctx.shadowBlur = 0;
			ctx.fill();
			}
		}

		// Walls.
		ctx.lineCap = 'round';
		ctx.lineJoin = 'round';
		ctx.shadowColor = COLORS.mucosa;
		ctx.shadowBlur = 16 + 10 * breath * (0.6 + 0.4 * Math.sin(t / 180));
		ctx.strokeStyle = COLORS.mucosa;
		ctx.lineWidth = 3.5;
		ctx.beginPath();
		curve(ctx, 0, n - 1, () => 0);
		ctx.stroke();
		ctx.beginPath();
		curve(ctx, 0, n - 1, dia);
		ctx.stroke();
		ctx.shadowBlur = 0;

		// Where the tract is heading.
		if (f) {
			ctx.setLineDash([2, 6]);
			ctx.lineWidth = 1.5;
			ctx.strokeStyle = 'rgba(243, 233, 247, 0.45)';
			ctx.beginPath();
			curve(ctx, 0, n - 1, (i) => lerp(f.targets, i));
			ctx.stroke();
			ctx.setLineDash([]);
		}

		// Tongue region and vowel marks.
		ctx.font = "600 17px 'Bricolage Grotesque Variable', sans-serif";
		ctx.textAlign = 'center';
		ctx.textBaseline = 'middle';
		for (const [ipa, , i, d] of VOWELS) {
			const p = point(i, d, lip);
			ctx.fillStyle = 'rgba(243, 233, 247, 0.5)';
			ctx.fillText(ipa, p.x, p.y);
		}

		const tongue = point(voice.tongue.index, voice.tongue.diameter, lip);
		const grabbed = [...grabs.values()].some((g) => g.kind === 'tongue');
		ctx.beginPath();
		ctx.arc(tongue.x, tongue.y, grabbed ? 15 : 12, 0, Math.PI * 2);
		ctx.fillStyle = 'rgba(255, 201, 77, 0.18)';
		ctx.fill();
		ctx.lineWidth = 3;
		ctx.strokeStyle = COLORS.brass;
		ctx.shadowColor = COLORS.brass;
		ctx.shadowBlur = 14;
		ctx.stroke();
		ctx.shadowBlur = 0;

		for (const g of grabs.values()) {
			if (g.kind !== 'touch') continue;
			ctx.beginPath();
			ctx.arc(g.at.x, g.at.y, 18, 0, Math.PI * 2);
			ctx.lineWidth = 2.5;
			ctx.strokeStyle = COLORS.brass;
			ctx.stroke();
		}

		// Landmarks.
		ctx.font = "500 14px 'Bricolage Grotesque Variable', sans-serif";
		ctx.fillStyle = COLORS.cartilage;
		const label = (text: string, i: number, d: number) => {
			const p = point(i, d, lip);
			ctx.fillText(text, p.x, p.y);
		};
		label('glottis', 0.5, -0.9);
		label('lips', n - 0.5, 2.3);
		label('nose', n - 1, -2.65);
		label('soft palate', noseStart - 3.5, -1.0);
		label('tongue', voice.tongue.index, voice.tongue.diameter + 0.55);
		ctx.textAlign = 'start';
		ctx.globalAlpha = 1;
	}

	$effect(() => {
		const ctx = canvas.getContext('2d');
		if (!ctx) return;
		let raf = 0;

		const resize = () => {
			const dpr = window.devicePixelRatio || 1;
			const { width, height } = canvas.getBoundingClientRect();
			canvas.width = Math.round(width * dpr);
			canvas.height = Math.round(height * dpr);
			const scale = Math.min(width / VIEW.width, height / VIEW.height);
			view = {
				scale,
				ox: (width - VIEW.width * scale) / 2,
				oy: (height - VIEW.height * scale) / 2
			};
		};
		const observer = new ResizeObserver(resize);
		observer.observe(canvas);
		resize();

		const frame = (t: number) => {
			const dpr = window.devicePixelRatio || 1;
			ctx.setTransform(1, 0, 0, 1, 0, 0);
			ctx.clearRect(0, 0, canvas.width, canvas.height);
			ctx.setTransform(
				dpr * view.scale,
				0,
				0,
				dpr * view.scale,
				dpr * (view.ox - VIEW.x * view.scale),
				dpr * (view.oy - VIEW.y * view.scale)
			);
			draw(ctx, t);
			raf = requestAnimationFrame(frame);
		};
		raf = requestAnimationFrame(frame);

		return () => {
			cancelAnimationFrame(raf);
			observer.disconnect();
		};
	});
</script>

<canvas
	bind:this={canvas}
	class:awake={voice.awake}
	onpointerdown={down}
	onpointermove={move}
	onpointerup={up}
	onpointercancel={up}
	aria-label="Vocal tract. Drag inside the tongue area to move the tongue, anywhere else along the throat to constrict it."
></canvas>

<style>
	canvas {
		display: block;
		width: 100%;
		height: 100%;
		touch-action: none;
		cursor: default;
	}

	canvas.awake {
		cursor: crosshair;
	}
</style>
