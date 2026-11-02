<script lang="ts">
	import Rail from '#lib/components/Rail.svelte';
	import Scope from '#lib/components/Scope.svelte';
	import Tract from '#lib/components/Tract.svelte';
	import Voicebox from '#lib/components/Voicebox.svelte';
	import { Voice } from '#lib/audio/voice.svelte.ts';

	const voice = new Voice();

	let drone = $state(true);
	let reverb = $state(0.2);
	let nose = $state(0);
	let wobble = $state(false);
	/** The sound buttons step through themselves until the first interaction. */
	let cycling = $state(false);
	let touched = false;

	function interacted() {
		touched = true;
		cycling = false;
	}

	// Settings that live only on this page; sent whenever they change.
	$effect(() => {
		if (voice.awake) voice.send({ type: 'drone', on: drone });
	});
	$effect(() => {
		if (voice.awake) voice.send({ type: 'reverb', value: reverb });
	});
	$effect(() => {
		if (voice.awake) voice.send({ type: 'velum', value: nose });
	});
	$effect(() => {
		if (voice.awake) voice.send({ type: 'wobble', on: wobble });
	});

	const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

	/** Resolve once `done()` holds, checking every 50 ms (gives up after `limit` ms). */
	async function until(done: () => boolean, limit = 8000) {
		for (let waited = 0; !done() && waited < limit; waited += 50) await wait(50);
	}

	async function wake() {
		// Hold the voice back during the introduction, so the line ends in a
		// short silence before the singing starts.
		drone = false;
		await voice.start();
		if (!voice.awake) return;
		voice.setNote(voice.baseNote);
		voice.speak('synthie trombone, yay!');
		await until(() => voice.speaking >= 0, 2000);
		await until(() => voice.speaking < 0);
		await wait(500);
		drone = true;
		if (!touched) cycling = true;
	}

	// The terminal app's piano row, starting at the C below the voice's pitch.
	const PIANO = 'awsedftgyhujkolp;';
	let octaveShift = $state(0);
	let held = $state<string | null>(null);

	function keydown(e: KeyboardEvent) {
		const target = e.target as HTMLElement | null;
		if (!voice.awake || e.metaKey || e.ctrlKey || e.altKey) return;
		if (target?.closest('textarea, input, select')) return;
		const key = e.key.toLowerCase();
		const semi = PIANO.indexOf(key);
		if (semi >= 0) {
			e.preventDefault();
			if (e.repeat) return;
			const c = voice.baseNote - (((voice.baseNote % 12) + 12) % 12) + 12 * octaveShift;
			held = key;
			voice.noteOn(c + semi);
		} else if (key === 'z') {
			octaveShift = Math.max(-2, octaveShift - 1);
		} else if (key === 'x') {
			octaveShift = Math.min(2, octaveShift + 1);
		} else if (key === ' ') {
			e.preventDefault();
			drone = !drone;
			voice.noteOff();
		}
	}

	function keyup(e: KeyboardEvent) {
		if (held && e.key.toLowerCase() === held) {
			held = null;
			voice.noteOff();
		}
	}
</script>

<svelte:head>
	<title>synthie trombone</title>
	<meta
		name="description"
		content="A physical model of the human voice you can play: shape the throat with your fingers, sing on the voicebox, or type a line and hear it spoken."
	/>
</svelte:head>

<svelte:window
	onkeydown={keydown}
	onkeyup={keyup}
	onpointerdowncapture={() => voice.awake && interacted()}
	onkeydowncapture={() => voice.awake && interacted()}
/>

<main class:awake={voice.awake}>
	<div class="stage">
		<Tract {voice} />
		{#if !voice.awake}
			<div class="wake">
				<p>Drag across the mouth to shape sounds, sing on the voicebox below, or type a line for it to say.</p>
				<button onclick={wake} disabled={voice.starting}>
					{voice.starting ? 'Starting…' : 'Turn on sound'}
				</button>
				{#if voice.error}<p class="error">{voice.error}</p>{/if}
			</div>
		{/if}
	</div>

	<div class="play">
		<div class="voicebox"><Voicebox {voice} /></div>
		<div class="scope"><Scope {voice} /></div>
	</div>

	<aside class="rail">
		<Rail {voice} bind:drone bind:reverb bind:nose bind:wobble bind:cycling />
		<footer>
			<p>
				Keys <kbd>a</kbd>–<kbd>;</kbd> play notes, <kbd>z</kbd>/<kbd>x</kbd> shift the octave,
				<kbd>Space</kbd> silences.
			</p>
			<p>
				After <a href="https://dood.al/pinktrombone/">Pink Trombone</a> by Neil Thapen. Rust and
				WebAssembly: <a href="https://github.com/sunsided/synthie-trombone">synthie-trombone</a>.
			</p>
		</footer>
	</aside>
</main>

<style>
	main {
		display: grid;
		grid-template-columns: minmax(0, 1fr) minmax(19rem, 23rem);
		grid-template-rows: minmax(0, 1fr) 8.5rem;
		grid-template-areas:
			'stage rail'
			'play rail';
		gap: 1.25rem;
		height: 100dvh;
		padding: 1.25rem;
	}

	.stage {
		grid-area: stage;
		position: relative;
		min-height: 0;
	}

	.play {
		grid-area: play;
		display: grid;
		grid-template-columns: minmax(0, 1fr) 13rem;
		gap: 1.25rem;
	}

	.scope {
		border-radius: 1.25rem;
		background: var(--throat-deep);
		overflow: hidden;
	}

	.rail {
		grid-area: rail;
		overflow-y: auto;
		padding-right: 0.25rem;
	}

	.wake {
		position: absolute;
		left: 50%;
		bottom: 12%;
		transform: translateX(-50%);
		width: min(26rem, 90%);
		text-align: center;
	}

	.wake p {
		margin: 0 0 1rem;
		font-size: 1.1rem;
		color: var(--bone);
	}

	.wake button {
		font: inherit;
		font-size: 1.15rem;
		font-weight: 700;
		color: var(--throat);
		background: var(--brass);
		border: none;
		border-radius: 999px;
		padding: 0.75rem 1.6rem;
		cursor: pointer;
		box-shadow: 0 0 32px rgb(255 201 77 / 0.35);
	}

	.wake button:focus-visible {
		outline: 2px solid var(--bone);
		outline-offset: 3px;
	}

	.error {
		color: var(--mucosa);
	}

	footer {
		padding-top: 1rem;
		border-top: 1px solid rgb(140 106 166 / 0.25);
		font-size: 0.85rem;
		color: var(--cartilage);
	}

	footer p {
		margin: 0 0 0.5rem;
	}

	footer a {
		color: var(--bone);
	}

	kbd {
		font: inherit;
		padding: 0 0.3rem;
		border: 1px solid rgb(243 233 247 / 0.2);
		border-radius: 0.3rem;
		color: var(--bone);
	}

	@media (max-width: 860px) {
		main {
			grid-template-columns: minmax(0, 1fr);
			grid-template-rows: min(70vh, 34rem) 8rem auto;
			grid-template-areas: 'stage' 'play' 'rail';
			height: auto;
			padding: 1rem;
		}

		.play {
			grid-template-columns: minmax(0, 1fr);
		}

		.scope {
			display: none;
		}

		.rail {
			overflow: visible;
		}
	}
</style>
