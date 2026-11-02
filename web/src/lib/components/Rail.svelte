<script lang="ts">
	import { untrack } from 'svelte';
	import type { Voice } from '#lib/audio/voice.svelte.ts';

	interface Props {
		voice: Voice;
		drone: boolean;
		reverb: number;
		nose: number;
		wobble: boolean;
		/** Step through the sounds on its own until someone interacts. */
		cycling: boolean;
	}

	let {
		voice,
		drone = $bindable(),
		reverb = $bindable(),
		nose = $bindable(),
		wobble = $bindable(),
		cycling = $bindable()
	}: Props = $props();

	const LINES = [
		'Hello, I am the synthie trombone!',
		'Are you still there?',
		'Shiny! Mine, all mine!'
	];

	/**
	 * Sounds to shape the tract with: label and ARPAbet symbols. With two
	 * symbols the shape glides from the first to the second ("eye").
	 */
	const SOUNDS: [string, ...string[]][] = [
		['ee', 'IY'],
		['ih', 'IH'],
		['eh', 'EH'],
		['a', 'AE'],
		['ah', 'AA'],
		['eye', 'AA', 'IY'],
		['aw', 'AO'],
		['oh', 'OW'],
		['oo', 'UW'],
		['er', 'ER'],
		['m', 'M'],
		['n', 'N'],
		['ng', 'NG'],
		['l', 'L'],
		['s', 'S'],
		['sh', 'SH'],
		['f', 'F'],
		['th', 'TH']
	];

	let text = $state('');
	/** Label of the sound shown as selected, picked by hand or by the cycle. */
	let current = $state<string | null>(null);
	let glide: ReturnType<typeof setTimeout> | undefined;

	function shape([label, first, ...rest]: [string, ...string[]]) {
		current = label;
		clearTimeout(glide);
		voice.articulate(first);
		// Diphthongs: move on to the next target after a short hold.
		rest.forEach((symbol, k) => {
			glide = setTimeout(() => voice.articulate(symbol), 220 * (k + 1));
		});
	}

	// Round and round through the sounds while nobody touches anything.
	$effect(() => {
		if (!cycling || !voice.awake) return;
		// Start from the selected sound; untracked so stepping doesn't restart the timer.
		let k = untrack(() => Math.max(0, SOUNDS.findIndex(([label]) => label === current)));
		untrack(() => shape(SOUNDS[k]));
		const timer = setInterval(() => {
			k = (k + 1) % SOUNDS.length;
			shape(SOUNDS[k]);
		}, 900);
		return () => clearInterval(timer);
	});

	/** Phonemes of the last line, with repeated segments merged. */
	let spoken = $derived.by(() => {
		const out: { symbol: string; from: number; to: number }[] = [];
		voice.symbols.forEach((symbol, i) => {
			const last = out.at(-1);
			if (last && last.symbol === symbol) last.to = i;
			else out.push({ symbol, from: i, to: i });
		});
		return out;
	});

	function say(line: string) {
		const t = line.trim();
		if (!t || !voice.awake) return;
		voice.speak(t);
	}

	function submit(e: SubmitEvent) {
		e.preventDefault();
		say(text);
	}

	function keydown(e: KeyboardEvent) {
		if (e.key === 'Enter' && !e.shiftKey) {
			e.preventDefault();
			say(text);
		}
	}
</script>

<section aria-labelledby="voice-heading">
	<h2 id="voice-heading">Voice</h2>
	<div class="chips" role="radiogroup" aria-labelledby="voice-heading">
		{#each voice.presets as name (name)}
			<button
				role="radio"
				aria-checked={voice.preset === name}
				class:on={voice.preset === name}
				onclick={() => voice.setPreset(name)}
				disabled={!voice.awake}
			>
				{name}
			</button>
		{/each}
		{#if !voice.presets.length}
			<p class="hint">Turn on sound to pick a voice.</p>
		{/if}
	</div>
</section>

<section aria-labelledby="say-heading">
	<h2 id="say-heading">Say something</h2>
	<form onsubmit={submit}>
		<textarea
			bind:value={text}
			onkeydown={keydown}
			rows="2"
			maxlength="400"
			placeholder="Type a line in English, or ARPAbet in [brackets]"
			disabled={!voice.awake}
		></textarea>
		<div class="row">
			<button type="submit" class="primary" disabled={!voice.awake || !text.trim()}>Say it</button>
			{#if voice.speaking >= 0}
				<button type="button" onclick={() => voice.stop()}>Stop</button>
			{/if}
		</div>
	</form>
	<div class="lines">
		{#each LINES as line (line)}
			<button class="line" onclick={() => say(line)} disabled={!voice.awake}>“{line}”</button>
		{/each}
	</div>
	{#if spoken.length}
		<p class="phonemes" aria-live="off">
			{#each spoken as p (p.from)}
				<span
					class:pause={p.symbol === 'SIL'}
					class:now={voice.speaking >= p.from && voice.speaking <= p.to}>{p.symbol === 'SIL' ? '·' : p.symbol}</span
				>
			{/each}
		</p>
	{/if}
</section>

<section aria-labelledby="shape-heading">
	<h2 id="shape-heading">Shape a sound</h2>
	<div class="sounds">
		{#each SOUNDS as sound (sound[0])}
			<button
				class:on={current === sound[0]}
				aria-pressed={current === sound[0]}
				onclick={() => shape(sound)}
				disabled={!voice.awake}
				title={sound.slice(1).join(' → ')}>{sound[0]}</button
			>
		{/each}
	</div>
</section>

<section aria-labelledby="tune-heading">
	<h2 id="tune-heading">Tune</h2>
	{#if voice.settings}
		{@const s = voice.settings}
		<label>
			<span>Throat length <output>{s.tractLength.toFixed(2)}</output></span>
			<input
				type="range"
				min="0.5"
				max="1.6"
				step="0.01"
				value={s.tractLength}
				oninput={(e) => voice.setSetting('tractLength', e.currentTarget.valueAsNumber)}
			/>
		</label>
		<label>
			<span>Tenseness <output>{s.tenseness.toFixed(2)}</output></span>
			<input
				type="range"
				min="0"
				max="1"
				step="0.01"
				value={s.tenseness}
				oninput={(e) => voice.setTenseness(e.currentTarget.valueAsNumber)}
			/>
		</label>
		<label>
			<span>Vibrato <output>{s.vibrato.toFixed(3)}</output></span>
			<input
				type="range"
				min="0"
				max="0.05"
				step="0.001"
				value={s.vibrato}
				oninput={(e) => voice.setVibrato(e.currentTarget.valueAsNumber)}
			/>
		</label>
		<label>
			<span>Speech rate <output>{s.rate.toFixed(2)}×</output></span>
			<input
				type="range"
				min="0.4"
				max="2.2"
				step="0.05"
				value={s.rate}
				oninput={(e) => voice.setSetting('rate', e.currentTarget.valueAsNumber)}
			/>
		</label>
	{/if}
	<label>
		<span>Nose <output>{nose.toFixed(2)}</output></span>
		<input type="range" min="0" max="1" step="0.01" bind:value={nose} disabled={!voice.awake} />
	</label>
	<label>
		<span>Room <output>{reverb.toFixed(2)}</output></span>
		<input type="range" min="0" max="0.8" step="0.01" bind:value={reverb} disabled={!voice.awake} />
	</label>
	<div class="toggles">
		<label class="toggle">
			<input type="checkbox" bind:checked={drone} disabled={!voice.awake} />
			<span>Keep the voice on</span>
		</label>
		<label class="toggle">
			<input type="checkbox" bind:checked={wobble} disabled={!voice.awake} />
			<span>Wander in pitch</span>
		</label>
	</div>
</section>

<style>
	section {
		padding: 1.1rem 0;
		border-top: 1px solid rgb(140 106 166 / 0.25);
	}

	section:first-child {
		border-top: none;
		padding-top: 0;
	}

	h2 {
		margin: 0 0 0.7rem;
		font-size: 1.05rem;
		font-weight: 700;
		font-stretch: 112%;
		color: var(--bone);
	}

	button {
		font: inherit;
		color: var(--bone);
		background: rgb(243 233 247 / 0.06);
		border: 1px solid rgb(243 233 247 / 0.14);
		border-radius: 999px;
		padding: 0.35rem 0.8rem;
		cursor: pointer;
		transition:
			background 120ms,
			border-color 120ms,
			color 120ms;
	}

	button:hover:not(:disabled) {
		border-color: rgb(255 79 154 / 0.6);
	}

	button:disabled {
		opacity: 0.4;
		cursor: default;
	}

	button:focus-visible,
	textarea:focus-visible,
	input:focus-visible {
		outline: 2px solid var(--brass);
		outline-offset: 2px;
	}

	.chips,
	.sounds {
		display: flex;
		flex-wrap: wrap;
		gap: 0.4rem;
	}

	.chips button.on {
		background: var(--mucosa);
		border-color: var(--mucosa);
		color: var(--throat);
		font-weight: 700;
	}

	.sounds button {
		min-width: 2.6rem;
		border-radius: 0.6rem;
	}

	.sounds button.on {
		border-color: var(--breath);
		color: var(--breath);
		box-shadow: 0 0 12px rgb(95 242 255 / 0.35);
	}

	.hint {
		margin: 0;
		color: var(--cartilage);
	}

	form {
		display: grid;
		gap: 0.5rem;
	}

	textarea {
		font: inherit;
		color: var(--bone);
		background: var(--throat-deep);
		border: 1px solid rgb(243 233 247 / 0.14);
		border-radius: 0.8rem;
		padding: 0.6rem 0.75rem;
		resize: vertical;
		min-height: 3.2rem;
	}

	textarea::placeholder {
		color: var(--cartilage);
	}

	.row {
		display: flex;
		gap: 0.5rem;
	}

	.primary {
		background: var(--brass);
		border-color: var(--brass);
		color: var(--throat);
		font-weight: 700;
	}

	.lines {
		display: flex;
		flex-direction: column;
		align-items: flex-start;
		gap: 0.15rem;
		margin-top: 0.6rem;
	}

	.line {
		background: none;
		border: none;
		padding: 0.15rem 0;
		color: var(--cartilage);
		text-align: left;
	}

	.line:hover:not(:disabled) {
		color: var(--bone);
	}

	.phonemes {
		display: flex;
		flex-wrap: wrap;
		gap: 0.25rem 0.45rem;
		margin: 0.8rem 0 0;
		font-variant-numeric: tabular-nums;
		color: var(--cartilage);
	}

	.phonemes span.pause {
		opacity: 0.5;
	}

	.phonemes span.now {
		color: var(--breath);
		text-shadow: 0 0 10px var(--breath);
	}

	label {
		display: grid;
		gap: 0.2rem;
		margin-bottom: 0.65rem;
	}

	label span {
		display: flex;
		justify-content: space-between;
		font-size: 0.92rem;
	}

	output {
		color: var(--cartilage);
		font-variant-numeric: tabular-nums;
	}

	input[type='range'] {
		width: 100%;
		accent-color: var(--mucosa);
	}

	.toggles {
		display: grid;
		gap: 0.3rem;
	}

	.toggle {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		margin: 0;
	}

	.toggle input {
		accent-color: var(--brass);
		width: 1.05rem;
		height: 1.05rem;
	}
</style>
