<script lang="ts">
	import type { Voice } from '#lib/audio/voice.svelte.ts';

	interface Props {
		voice: Voice;
	}

	let { voice }: Props = $props();

	const NAMES = ['C', 'C♯', 'D', 'D♯', 'E', 'F', 'F♯', 'G', 'G♯', 'A', 'A♯', 'B'];
	const SPAN = 24;

	let pad: HTMLDivElement;
	let pointer = $state<{ x: number; y: number } | null>(null);

	/** Lowest note on the pad: a fifth below the voice's speaking pitch, on a C or F. */
	let low = $derived(voice.baseNote - 12);
	let keys = $derived(
		Array.from({ length: SPAN + 1 }, (_, k) => {
			const note = low + k;
			const pc = ((note % 12) + 12) % 12;
			return {
				note,
				black: [1, 3, 6, 8, 10].includes(pc),
				// Octave names, except under the axis captions on the right.
				label: pc === 0 && k < SPAN - 2 ? `${NAMES[pc]}${Math.floor(note / 12) - 1}` : null
			};
		})
	);

	/** Where the voice is sounding now, as a fraction across the pad. */
	let marker = $derived.by(() => {
		if (!voice.frequency || voice.intensity < 0.02) return null;
		const note = 69 + 12 * Math.log2(voice.frequency / 440);
		return Math.min(1, Math.max(0, (note - low) / SPAN));
	});

	function read(e: PointerEvent) {
		const rect = pad.getBoundingClientRect();
		const x = Math.min(1, Math.max(0, (e.clientX - rect.left) / rect.width));
		const y = Math.min(1, Math.max(0, (e.clientY - rect.top) / rect.height));
		pointer = { x, y };
		return { note: low + x * SPAN, tenseness: 1 - y };
	}

	function down(e: PointerEvent) {
		if (!voice.awake) return;
		pad.setPointerCapture(e.pointerId);
		const { note, tenseness } = read(e);
		voice.setTenseness(tenseness);
		voice.noteOn(note);
	}

	function move(e: PointerEvent) {
		if (!pointer) return;
		const { note, tenseness } = read(e);
		voice.setTenseness(tenseness);
		voice.setNote(note);
	}

	function up() {
		if (!pointer) return;
		pointer = null;
		voice.noteOff();
	}
</script>

<div
	class="pad"
	bind:this={pad}
	onpointerdown={down}
	onpointermove={move}
	onpointerup={up}
	onpointercancel={up}
	role="slider"
	aria-label="Voicebox: pitch left to right, tenseness bottom to top"
	aria-valuemin={low}
	aria-valuemax={low + SPAN}
	aria-valuenow={voice.baseNote}
	tabindex="-1"
>
	{#each keys as key (key.note)}
		<div class="key" class:black={key.black} style:left="{((key.note - low) / SPAN) * 100}%">
			{#if key.label}<span>{key.label}</span>{/if}
		</div>
	{/each}
	<span class="axis breathy">breathy</span>
	<span class="axis pressed">pressed</span>
	{#if marker !== null}
		<div class="now" style:left="{marker * 100}%" style:opacity={Math.min(1, voice.intensity * 1.5)}></div>
	{/if}
	{#if pointer}
		<div class="finger" style:left="{pointer.x * 100}%" style:top="{pointer.y * 100}%"></div>
	{/if}
</div>

<style>
	.pad {
		position: relative;
		height: 100%;
		min-height: 5.5rem;
		border-radius: 1.25rem;
		background:
			linear-gradient(to top, rgb(140 106 166 / 0.18), rgb(255 79 154 / 0.14)),
			var(--throat-deep);
		overflow: hidden;
		touch-action: none;
		cursor: pointer;
	}

	.key {
		position: absolute;
		top: 0;
		bottom: 0;
		width: 1px;
		background: rgb(243 233 247 / 0.12);
	}

	.key.black {
		background: rgb(243 233 247 / 0.05);
	}

	.key span {
		position: absolute;
		top: 0.4rem;
		left: 0.35rem;
		font-size: 0.75rem;
		font-variant-numeric: tabular-nums;
		color: var(--cartilage);
	}

	.axis {
		position: absolute;
		right: 0.75rem;
		font-size: 0.75rem;
		color: var(--cartilage);
		pointer-events: none;
	}

	.breathy {
		bottom: 0.4rem;
	}

	.pressed {
		top: 0.4rem;
	}

	.now {
		position: absolute;
		top: 0;
		bottom: 0;
		width: 3px;
		margin-left: -1.5px;
		background: var(--breath);
		box-shadow: 0 0 14px var(--breath);
		pointer-events: none;
	}

	.finger {
		position: absolute;
		width: 2.4rem;
		height: 2.4rem;
		margin: -1.2rem 0 0 -1.2rem;
		border: 3px solid var(--brass);
		border-radius: 50%;
		box-shadow: 0 0 18px rgb(255 201 77 / 0.6);
		pointer-events: none;
	}
</style>
