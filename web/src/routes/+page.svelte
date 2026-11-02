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
		<a class="repo" href="https://github.com/sunsided/synthie-trombone" aria-label="synthie-trombone on GitHub">
			<!-- GitHub mark, Primer Octicons (MIT). -->
			<svg viewBox="0 0 16 16" width="28" height="28" aria-hidden="true">
				<path
					fill="currentColor"
					d="M8 0c4.42 0 8 3.58 8 8a8.013 8.013 0 0 1-5.45 7.59c-.4.08-.55-.17-.55-.38 0-.27.01-1.13.01-2.2 0-.75-.25-1.23-.54-1.48 1.78-.2 3.65-.88 3.65-3.95 0-.88-.31-1.59-.82-2.15.08-.2.36-1.02-.08-2.12 0 0-.67-.22-2.2.82-.64-.18-1.32-.27-2-.27-.68 0-1.36.09-2 .27-1.53-1.03-2.2-.82-2.2-.82-.44 1.1-.16 1.92-.08 2.12-.51.56-.82 1.28-.82 2.15 0 3.06 1.86 3.75 3.64 3.95-.23.2-.44.55-.51 1.07-.46.21-1.61.55-2.33-.66-.15-.24-.6-.83-1.23-.82-.67.01-.27.38.01.53.34.19.73.9.82 1.13.16.45.68 1.31 2.69.94 0 .67.01 1.3.01 1.49 0 .21-.15.45-.55.38A7.995 7.995 0 0 1 0 8c0-4.42 3.58-8 8-8Z"
				/>
			</svg>
		</a>
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

	.repo {
		position: absolute;
		top: 0.25rem;
		left: 0.25rem;
		display: grid;
		place-items: center;
		width: 2.75rem;
		height: 2.75rem;
		border-radius: 50%;
		color: var(--cartilage);
		transition: color 120ms;
		isolation: isolate;
	}

	/* Rays in synthie's rainbow, radiating around the mark: a faint glimmer
	   at rest, bright and slowly turning on hover or focus. */
	.repo::before {
		content: '';
		position: absolute;
		inset: -1rem;
		z-index: -1;
		border-radius: 50%;
		background: repeating-conic-gradient(
			from 0deg,
			#00f0ff 0deg 3deg,
			transparent 3deg 15deg,
			#a85cff 15deg 18deg,
			transparent 18deg 30deg,
			#ff2eaa 30deg 33deg,
			transparent 33deg 45deg,
			#ff8c28 45deg 48deg,
			transparent 48deg 60deg
		);
		mask: radial-gradient(circle, transparent 34%, #000 40%, #000 52%, transparent 70%);
		opacity: 0.18;
		transform: scale(0.8);
		transition:
			opacity 200ms ease-out,
			transform 300ms ease-out;
	}

	.repo:hover::before,
	.repo:focus-visible::before {
		opacity: 1;
		transform: scale(1);
		animation: rays 8s linear infinite;
	}

	@keyframes rays {
		to {
			rotate: 360deg;
		}
	}

	.repo:hover,
	.repo:focus-visible {
		color: var(--bone);
	}

	.repo:focus-visible {
		outline: 2px solid var(--brass);
		outline-offset: 2px;
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
