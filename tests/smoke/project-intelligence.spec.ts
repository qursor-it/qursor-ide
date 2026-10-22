import { expect, type Page, test } from '@playwright/test';
import {
	candidateFixture,
	evidenceFixture,
	FOO_ID,
	fixtureIntelligence,
	type IntelligenceFixture,
	INTEL_BASE,
	INTEL_DIFF,
	INTEL_FILE,
	INTEL_PROPOSED,
	installMockBridge,
	preparedFixture,
	runFixture,
	sha256,
	testSuggestionFixture,
} from './fixtures';

/**
 * Project Intelligence (specs/05-features.md F33, ADR-0084).
 *
 * What these pin is the contract between a person and the feature, not its
 * layout: nothing is read while the switch is off; nothing is sent without the
 * consent given for that run; a project file is written only by a click on a
 * Save, and a change is recorded only after one; and a suggestion never claims
 * more than it has - a single session is a hypothesis, an implemented change is
 * unverified until a person says otherwise, and a run with no model behind it is
 * a list of unreviewed observations.
 *
 * The backend is the mock bridge. Its prepared changes carry real sha256 hashes
 * (`sha256` in the fixtures), so a spec can assert that the renderer reports the
 * hash of what it saved.
 */

const TAB = `/#/projects/${FOO_ID}?tab=intelligence`;

/** The mocked command calls named `name`, with their arguments, in order. */
function calls(page: Page, name: string) {
	return page.evaluate(
		(n) =>
			(window.__FACTORAI_TEST_CALLS__ ?? []).filter((c) => c.name === n).map((c) => c.args ?? {}),
		name,
	);
}

/** Every mocked command called so far, by name. */
function callNames(page: Page) {
	return page.evaluate(() => (window.__FACTORAI_TEST_CALLS__ ?? []).map((c) => c.name));
}

function emit(page: Page, event: string, payload: unknown) {
	return page.evaluate(([e, p]) => window.__FACTORAI_EMIT__?.(e as string, p), [event, payload]);
}

/** A fixture whose latest run completed with these suggestions. */
function withResults(
	candidates: ReturnType<typeof candidateFixture>[],
	extra: IntelligenceFixture = {},
) {
	return fixtureIntelligence({
		latestRuns: {
			[FOO_ID]: { run: runFixture({ candidateCount: candidates.length }), signals: [] },
		},
		improvements: { [FOO_ID]: candidates },
		...extra,
	});
}

async function openTab(page: Page, fixture: ReturnType<typeof fixtureIntelligence>) {
	await installMockBridge(page, fixture);
	await page.goto(TAB);
	await expect(page.getByTestId('intelligence-view')).toBeVisible();
}

/** Open a suggestion's detail. */
async function expand(page: Page, id: string) {
	const card = page.getByTestId(`candidate-${id}`);
	await card.getByTestId('candidate-toggle').click();
	await expect(card.getByTestId('candidate-detail')).toBeVisible();
	return card;
}

const implemented = () =>
	candidateFixture({
		status: 'implemented',
		implementation: {
			targetPath: INTEL_FILE,
			relativePath: 'AGENTS.md',
			beforeHash: sha256(INTEL_BASE),
			afterHash: sha256(INTEL_PROPOSED),
			createdFile: false,
			implementedAt: Date.now() - 30_000,
		},
		events: [
			{
				id: 1,
				eventType: 'proposed',
				outcome: null,
				method: null,
				note: null,
				artifactPath: null,
				beforeHash: null,
				afterHash: null,
				createdAt: Date.now() - 600_000,
			},
			{
				id: 2,
				eventType: 'implemented',
				outcome: null,
				method: null,
				note: null,
				artifactPath: 'AGENTS.md',
				beforeHash: sha256(INTEL_BASE),
				afterHash: sha256(INTEL_PROPOSED),
				createdAt: Date.now() - 30_000,
			},
		],
	});

test.describe('project intelligence', () => {
	test('@smoke no tab while the switch is off, and ?tab= falls back to sessions', async ({
		page,
	}) => {
		await installMockBridge(page, { ...fixtureIntelligence(), settings: {} });
		await page.goto(TAB);

		await expect(page.getByTestId('project-tab-sessions')).toHaveAttribute('aria-selected', 'true');
		await expect(page.getByTestId('project-tab-intelligence')).toHaveCount(0);
		await expect(page.getByTestId('intelligence-view')).toHaveCount(0);
		await expect(page.getByText('Refactor the auth middleware')).toBeVisible();

		// Off means nothing is read: the only thing asked of the backend is the
		// switch itself.
		const names = await callNames(page);
		for (const forbidden of [
			'preview_analysis',
			'start_analysis',
			'list_improvements',
			'latest_intelligence_run',
		]) {
			expect(names).not.toContain(forbidden);
		}
	});

	test('@smoke the switch is turned on in Settings, shows the provider, and the tab appears', async ({
		page,
	}) => {
		await installMockBridge(page, { ...fixtureIntelligence(), settings: {} });
		await page.goto(`/#/projects/${FOO_ID}`);
		await expect(page.getByTestId('project-tab-intelligence')).toHaveCount(0);

		await page.getByTestId('open-settings').click();
		await page.getByTestId('settings-nav-intelligence').click();
		const sw = page.getByTestId('settings-intelligence');
		await expect(sw).toHaveAttribute('aria-checked', 'false');
		// Off by default, and the provider is not probed or described while off.
		await expect(page.getByTestId('settings-intelligence-provider')).toHaveCount(0);
		// The consent and Save rules are stated before the switch is flipped.
		const privacy = page.getByTestId('settings-intelligence-privacy');
		await expect(privacy).toContainText('consent for each run');
		await expect(privacy).toContainText('only by a Save you press');

		await sw.click();
		await expect(sw).toHaveAttribute('aria-checked', 'true');
		await expect(page.getByTestId('settings-intelligence-provider')).toHaveText('available');
		await expect(page.getByTestId('settings-modal')).toContainText('It is remote');

		// It wrote as it was clicked: the modal's Save is not involved.
		expect(await calls(page, 'set_setting')).toEqual([{ key: 'projectIntelligence', value: 'on' }]);
		await expect(page.getByTestId('settings-save')).toBeDisabled();

		await page.keyboard.press('Escape');
		await expect(page.getByTestId('project-tab-intelligence')).toBeVisible();

		// And off again takes the tab with it.
		await page.getByTestId('open-settings').click();
		await page.getByTestId('settings-nav-intelligence').click();
		await page.getByTestId('settings-intelligence').click();
		await expect(page.getByTestId('settings-intelligence')).toHaveAttribute(
			'aria-checked',
			'false',
		);
		expect((await calls(page, 'set_setting')).at(-1)).toEqual({
			key: 'projectIntelligence',
			value: null,
		});
		await page.keyboard.press('Escape');
		await expect(page.getByTestId('project-tab-intelligence')).toHaveCount(0);
	});

	test('@smoke an unavailable provider is stated in Settings and in the preview', async ({
		page,
	}) => {
		await installMockBridge(
			page,
			fixtureIntelligence({
				provider: {
					id: 'claude_cli',
					displayName: 'Claude Code CLI',
					remote: true,
					available: false,
					version: null,
					binaryPath: null,
					reason: 'not_installed',
					detail: null,
				},
			}),
		);
		await page.goto('/?settings=intelligence');
		await expect(page.getByTestId('settings-intelligence-provider')).toHaveText('unavailable');
		await expect(page.getByTestId('settings-modal')).toContainText('is not installed');
		await page.keyboard.press('Escape');

		await page.goto(TAB);
		await page.getByTestId('intelligence-start').click();
		const preview = page.getByTestId('intelligence-preview');
		await expect(preview.getByTestId('intelligence-provider-available')).toHaveText('unavailable');
		// Nothing can be sent, so there is no consent to give.
		await expect(preview.getByTestId('intelligence-consent')).toHaveCount(0);
		await expect(preview.getByTestId('intelligence-no-send')).toContainText('nothing can be sent');
		await expect(preview.getByTestId('intelligence-analyze')).toHaveText(
			'Analyze offline (signals only)',
		);
	});

	test('@smoke empty, then a preview, then running, then results', async ({ page }) => {
		await openTab(page, fixtureIntelligence());
		await expect(page.getByTestId('project-tab-intelligence')).toHaveAttribute(
			'aria-selected',
			'true',
		);

		// Empty: one way in, and nothing has been asked of a model.
		await page.getByTestId('intelligence-start').click();

		const preview = page.getByTestId('intelligence-preview');
		await expect(preview.getByTestId('intelligence-session-count')).toContainText('2 completed');
		await expect(preview.getByTestId('intelligence-session-count')).toContainText('Claude 2');
		await expect(preview.getByTestId('intelligence-overview')).toContainText('1 still running');
		await expect(preview.getByTestId('intelligence-payload')).toContainText(
			'3 found, about 1.5 KB',
		);
		// A local command is not local inference, and the preview says so.
		await expect(preview.getByTestId('intelligence-provider-remote')).toHaveText('remote');
		await expect(preview.getByTestId('intelligence-provider')).toContainText('not local inference');
		await expect(preview.getByTestId('intelligence-signal')).toHaveCount(3);
		await expect(preview.getByTestId('intelligence-selected')).toHaveText(
			'3 of 3 excerpts selected',
		);
		await expect(preview.getByTestId('intelligence-session-limit')).toHaveValue('20');

		// Without consent the only run on offer is the offline one, named as such.
		const analyze = preview.getByTestId('intelligence-analyze');
		await expect(analyze).toHaveText('Analyze offline (signals only)');
		await expect(preview.getByTestId('intelligence-mode-note')).toContainText('Nothing is sent');

		// Strike the third excerpt, then agree to send the other two.
		await preview.getByTestId('intelligence-signal-include').nth(2).click();
		await expect(preview.getByTestId('intelligence-selected')).toHaveText(
			'2 of 3 excerpts selected',
		);
		await preview.getByTestId('intelligence-consent').click();
		await expect(analyze).toHaveText('Analyze');
		await expect(
			preview.getByText('Send the 2 selected excerpts to Claude Code CLI'),
		).toBeVisible();
		await analyze.click();

		// The request is exactly what was shown and agreed to.
		const started = await calls(page, 'start_analysis');
		expect(started).toHaveLength(1);
		expect(started[0]).toEqual({
			request: {
				projectId: FOO_ID,
				sessionLimit: 20,
				previewHash: 'hash-1',
				consentRemote: true,
				excludedEvidenceIds: ['ev-3'],
				includeInstructionFile: false,
			},
		});

		// Running: progress from the event, a way to cancel, and no file written.
		const running = page.getByTestId('intelligence-running');
		await expect(running).toBeVisible();
		await expect(running).toContainText('No file in your project is written');
		await emit(page, 'intelligence:progress', {
			runId: 'mock-run',
			projectId: FOO_ID,
			phase: 'analyzing',
			sessionsDone: 1,
			sessionsTotal: 2,
			signals: 2,
		});
		await expect(page.getByTestId('intelligence-running-phase')).toHaveText(
			'Analyzing sessions · 1 of 2 sessions · 2 signals',
		);
		await expect(running.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '50');
		// Another project's progress is not this one's.
		await emit(page, 'intelligence:progress', {
			runId: 'other',
			projectId: 'someone-else',
			phase: 'saving',
			sessionsDone: 9,
			sessionsTotal: 9,
			signals: 9,
		});
		await expect(page.getByTestId('intelligence-running-phase')).toContainText('1 of 2 sessions');

		// The run ends: the backend's row and candidates are what the tab reads.
		await page.evaluate(
			([run, improvements]) => {
				const fx = window.__FACTORAI_TEST__;
				if (!fx?.intelligence) return;
				fx.intelligence.latestRuns = { [run.projectId]: { run, signals: [] } };
				fx.intelligence.improvements = { [run.projectId]: improvements };
			},
			[runFixture({ id: 'mock-run' }), [candidateFixture(), testSuggestionFixture()]] as const,
		);
		await emit(page, 'intelligence:run-finished', {
			runId: 'mock-run',
			projectId: FOO_ID,
			state: 'completed',
			outcome: 'candidates',
			errorCode: null,
			candidateCount: 2,
			signalCount: 3,
		});

		await expect(page.getByTestId('intelligence-running')).toHaveCount(0);
		const results = page.getByTestId('intelligence-results');
		await expect(results).toBeVisible();
		await expect(page.getByTestId('intelligence-last-run')).toContainText('2 sessions · 3 signals');
		// Labelled by the kind of remedy, in a fixed order.
		await expect(page.getByTestId('group-instruction')).toContainText('Instruction');
		await expect(page.getByTestId('group-test_suggestion')).toContainText('Test suggestion');
		const instruction = page.getByTestId('candidate-cand-instruction');
		await expect(instruction.getByTestId('candidate-action')).toHaveText('Instruction');
		await expect(instruction.getByTestId('candidate-sessions')).toHaveText(
			'2 independent sessions',
		);
		// One session is a hypothesis, and called one.
		await expect(
			page.getByTestId('candidate-cand-test').getByTestId('candidate-sessions'),
		).toHaveText('hypothesis · 1 independent session');
		// Nothing on the page is a score.
		await expect(results).not.toContainText('%');
		await expect(results).not.toContainText('confidence');
	});

	test('@smoke without consent the run is offline and sends nothing', async ({ page }) => {
		await openTab(page, fixtureIntelligence());
		await page.getByTestId('intelligence-start').click();
		await page.getByTestId('intelligence-analyze').click();
		const started = await calls(page, 'start_analysis');
		expect(started).toHaveLength(1);
		expect(started[0]).toMatchObject({
			request: { consentRemote: false, excludedEvidenceIds: [], includeInstructionFile: false },
		});
	});

	test('@smoke Cancel on the preview starts nothing', async ({ page }) => {
		await openTab(page, fixtureIntelligence());
		await page.getByTestId('intelligence-start').click();
		await page.getByTestId('intelligence-preview-cancel').click();
		await expect(page.getByTestId('intelligence-preview')).toHaveCount(0);
		await expect(page.getByTestId('intelligence-start')).toBeVisible();
		expect(await calls(page, 'start_analysis')).toHaveLength(0);
	});

	test('@smoke a stale preview is refused, re-read, and the consent is withdrawn', async ({
		page,
	}) => {
		await openTab(
			page,
			fixtureIntelligence({
				fails: { kind: 'Intelligence', message: 'preview_stale: the transcripts changed' },
			}),
		);
		await page.getByTestId('intelligence-start').click();
		const preview = page.getByTestId('intelligence-preview');
		await preview.getByTestId('intelligence-consent').click();
		await expect(preview.getByTestId('intelligence-consent')).toHaveAttribute(
			'aria-checked',
			'true',
		);
		await preview.getByTestId('intelligence-analyze').click();

		await expect(preview.getByTestId('intelligence-start-error')).toContainText(
			'The sessions changed after the preview',
		);
		// Re-read, so what is on screen is what a new run would send...
		await expect.poll(async () => (await calls(page, 'preview_analysis')).length).toBe(2);
		// ...and consent given for the old set no longer stands.
		await expect(preview.getByTestId('intelligence-consent')).toHaveAttribute(
			'aria-checked',
			'false',
		);
		await expect(preview.getByTestId('intelligence-analyze')).toHaveText(
			'Analyze offline (signals only)',
		);
	});

	test('@smoke no actionable patterns shows a result and invents nothing', async ({ page }) => {
		await openTab(
			page,
			fixtureIntelligence({
				latestRuns: {
					[FOO_ID]: { run: runFixture({ outcome: 'no_change', candidateCount: 0 }), signals: [] },
				},
			}),
		);
		const none = page.getByTestId('intelligence-no-change');
		await expect(none).toContainText('No actionable patterns');
		await expect(none).toContainText('Nothing was added to your project');
		await expect(page.getByTestId('candidate-toggle')).toHaveCount(0);
		await expect(page.getByTestId('intelligence-unreviewed')).toHaveCount(0);
	});

	test('@smoke signals only are unreviewed observations, not insights', async ({ page }) => {
		await openTab(
			page,
			fixtureIntelligence({
				latestRuns: {
					[FOO_ID]: {
						run: runFixture({
							outcome: 'signals_only',
							provider: null,
							candidateCount: 0,
							signalCount: 2,
							errorCode: 'provider_unavailable',
						}),
						signals: [
							evidenceFixture(),
							evidenceFixture({
								evidenceId: 'ev-2',
								sessionAvailable: false,
								excerpt: null,
								context: null,
							}),
						],
					},
				},
			}),
		);
		const unreviewed = page.getByTestId('intelligence-unreviewed');
		await expect(unreviewed).toContainText('Unreviewed signals (2)');
		await expect(unreviewed).toContainText('not findings');
		await expect(unreviewed).toContainText('nothing was suggested');
		await expect(unreviewed.getByTestId('evidence-item')).toHaveCount(2);
		await expect(unreviewed.getByTestId('evidence-unavailable')).toHaveCount(1);
		// Asked for and not possible: said out loud, and not as a failure.
		const degraded = page.getByTestId('intelligence-run-degraded');
		await expect(degraded).toContainText('no model reviewed these');
		await expect(degraded).toHaveAttribute('role', 'status');
		// No candidate, no group, no action.
		await expect(page.getByTestId('candidate-toggle')).toHaveCount(0);
		await expect(page.getByTestId('intelligence-review')).toHaveCount(0);
		await expect(page.getByText('Prepare change')).toHaveCount(0);
	});

	test('@smoke a failed run says what happened and offers another try', async ({ page }) => {
		await openTab(
			page,
			fixtureIntelligence({
				latestRuns: {
					[FOO_ID]: {
						run: runFixture({
							state: 'failed',
							outcome: null,
							candidateCount: 0,
							errorCode: 'timeout',
						}),
						signals: [],
					},
				},
			}),
		);
		const error = page.getByTestId('intelligence-run-error');
		await expect(error).toContainText('The analysis took too long');
		await expect(error).toContainText('nothing was saved');
		await error.getByTestId('intelligence-notice-retry').click();
		await expect(page.getByTestId('intelligence-preview')).toBeVisible();
	});

	test('@smoke a command refused because the switch is off says so and points at Settings', async ({
		page,
	}) => {
		await openTab(
			page,
			fixtureIntelligence({
				fails: { kind: 'Intelligence', message: 'disabled: Project Intelligence is off' },
				improvements: { [FOO_ID]: [candidateFixture()] },
				latestRuns: { [FOO_ID]: { run: runFixture(), signals: [] } },
			}),
		);
		const card = await expand(page, 'cand-instruction');
		await card.getByTestId('follow-up').click();
		const notice = card.getByTestId('intelligence-notice');
		await expect(notice).toContainText('Project Intelligence is off');
		await notice.getByTestId('intelligence-notice-settings').click();
		await expect(page.getByTestId('settings-modal')).toBeVisible();
		await expect(page.getByTestId('settings-nav-intelligence')).toHaveAttribute(
			'aria-current',
			'page',
		);
	});

	test('@smoke a suggestion shows its evidence, opens the session, and is keyboard reachable', async ({
		page,
	}) => {
		const gone = evidenceFixture({
			evidenceId: 'ev-2',
			sessionAvailable: false,
			excerpt: null,
			context: null,
			sessionTitle: 'Wire the settings modal',
		});
		await openTab(
			page,
			withResults([
				candidateFixture({
					evidence: [evidenceFixture(), gone],
					duplicate: {
						path: 'CLAUDE.md',
						line: 7,
						text: 'Always use pnpm.',
					},
					downgradeNote: null,
				}),
			]),
		);

		// Collapsed, it shows what lets you choose: the kind, the support, the title.
		const card = page.getByTestId('candidate-cand-instruction');
		await expect(card.getByTestId('candidate-duplicate')).toHaveText('may be a duplicate');
		await expect(card.getByTestId('candidate-detail')).toHaveCount(0);

		// Reachable and operable from the keyboard alone.
		await card.getByTestId('candidate-toggle').focus();
		await page.keyboard.press('Enter');
		const detail = card.getByTestId('candidate-detail');
		await expect(detail).toBeVisible();
		await expect(card.getByTestId('candidate-toggle')).toHaveAttribute('aria-expanded', 'true');

		await expect(detail).toContainText('The agent ran npm in two sessions');
		await expect(detail.getByTestId('candidate-recommendation')).toHaveText('Use pnpm, not npm.');
		await expect(detail).toContainText('Why this remedy');
		await expect(detail).toContainText('Limits of this inference');
		await expect(detail.getByTestId('candidate-duplicate-warning')).toContainText('CLAUDE.md:7');

		const items = detail.getByTestId('evidence-item');
		await expect(items).toHaveCount(2);
		await expect(items.first().getByTestId('evidence-excerpt')).toHaveText(
			'No, use pnpm here, never npm.',
		);
		// Gone with its session: said, and no words shown in their place.
		await expect(items.nth(1).getByTestId('evidence-unavailable')).toContainText(
			'Evidence no longer available',
		);
		await expect(items.nth(1).getByTestId('evidence-excerpt')).toHaveCount(0);
		await expect(items.nth(1).getByTestId('evidence-session-link')).toHaveCount(0);
		await expect(detail.getByTestId('candidate-timeline')).toContainText('Proposed');

		await items.first().getByTestId('evidence-session-link').click();
		await expect(page).toHaveURL(/\/projects\/[^/]+\/sessions\/session-uuid-001/);
	});

	test('@smoke dismissing asks first and closes the suggestion for good', async ({ page }) => {
		await openTab(page, withResults([candidateFixture(), testSuggestionFixture()]));
		const card = await expand(page, 'cand-instruction');

		await card.getByTestId('dismiss').click();
		const confirm = card.getByTestId('dismiss-confirm');
		await expect(confirm).toContainText('closed for good');
		// Asking is not doing.
		expect(await calls(page, 'dismiss_improvement')).toHaveLength(0);
		await confirm.getByLabel('Reason (optional)').fill('covered by the README');
		await confirm.getByTestId('dismiss-confirm-ok').click();

		expect(await calls(page, 'dismiss_improvement')).toEqual([
			{ candidateId: 'cand-instruction', reason: 'covered by the README' },
		]);
		// It left the list to review and is among the closed.
		await expect(page.getByTestId('candidate-cand-instruction')).toHaveCount(0);
		await expect(page.getByTestId('intelligence-closed-toggle')).toHaveText('Show 1 closed');
		await page.getByTestId('intelligence-closed-toggle').click();
		const closed = page.getByTestId('candidate-cand-instruction');
		await expect(closed).toHaveAttribute('data-status', 'dismissed');
		await closed.getByTestId('candidate-toggle').click();
		await expect(closed.getByTestId('candidate-timeline')).toContainText('covered by the README');
		await expect(closed.getByTestId('prepare-change')).toHaveCount(0);
		await expect(closed.getByTestId('dismiss')).toHaveCount(0);
	});

	test('@smoke a test suggestion is consult-only: no prepare, no run, a follow-up flag', async ({
		page,
	}) => {
		await openTab(page, withResults([testSuggestionFixture()]));
		const card = page.getByTestId('candidate-cand-test');
		await expect(card.getByTestId('candidate-sessions')).toHaveText(
			'hypothesis · 1 independent session',
		);
		await card.getByTestId('candidate-toggle').click();

		await expect(card.getByTestId('consult-only')).toContainText('Nothing is run or written');
		await expect(card.getByTestId('prepare-change')).toHaveCount(0);
		await expect(card.getByTestId('candidate-downgrade')).toContainText('only one session');
		await expect(card.getByTestId('candidate-detail')).toContainText('one session is a hypothesis');

		await card.getByTestId('follow-up').click();
		expect(await calls(page, 'mark_follow_up')).toEqual([
			{ candidateId: 'cand-test', followUp: true },
		]);
		await expect(card.getByTestId('candidate-follow-up')).toBeVisible();
		await expect(card.getByTestId('follow-up')).toHaveText('Remove follow-up flag');
		await expect(card.getByTestId('follow-up')).toHaveAttribute('aria-pressed', 'true');

		// Nothing was written, run, or prepared.
		const names = await callNames(page);
		expect(names).not.toContain('write_file');
		expect(names).not.toContain('prepare_instruction_change');
	});

	test('@smoke an instruction is prepared as a diff that writes nothing until Save', async ({
		page,
	}) => {
		await openTab(
			page,
			withResults([candidateFixture()], {
				prepared: { 'cand-instruction': preparedFixture() },
			}),
		);
		const card = await expand(page, 'cand-instruction');
		await card.getByTestId('prepare-change').click();

		const prepared = card.getByTestId('prepare-instruction');
		await expect(prepared.getByTestId('prepare-target')).toHaveText(INTEL_FILE);
		await expect(prepared.getByTestId('intelligence-diff')).toContainText('+- Use pnpm, not npm.');
		await expect(prepared.getByTestId('intelligence-diff')).toContainText('+2');
		await expect(prepared.getByTestId('prepare-nothing-written')).toContainText(
			'Nothing has been written',
		);
		expect(await calls(page, 'prepare_instruction_change')).toEqual([
			{ candidateId: 'cand-instruction' },
		]);

		// Preparing is not saving, and neither is asking to save.
		await prepared.getByTestId('save-directly').click();
		const dialog = page.getByTestId('intelligence-save-dialog');
		await expect(dialog).toContainText('Save to AGENTS.md?');
		await expect(dialog).toContainText(INTEL_FILE);
		expect(await calls(page, 'write_file')).toHaveLength(0);
		await dialog.getByRole('button', { name: 'Cancel' }).click();
		await expect(dialog).toHaveCount(0);
		expect(await calls(page, 'write_file')).toHaveLength(0);
		expect(await calls(page, 'record_implementation')).toHaveLength(0);
		await expect(card).toHaveAttribute('data-status', 'proposed');
	});

	test('@smoke Save in the dialog writes the file, records the hash, and leaves it unverified', async ({
		page,
	}) => {
		await openTab(
			page,
			withResults([candidateFixture()], {
				prepared: { 'cand-instruction': preparedFixture() },
			}),
		);
		const card = await expand(page, 'cand-instruction');
		await card.getByTestId('prepare-change').click();
		await card.getByTestId('save-directly').click();
		await page.getByTestId('intelligence-confirm-save').click();

		// Written first, with exactly the proposal; recorded second, with its hash.
		expect(await calls(page, 'write_file')).toEqual([
			{ path: INTEL_FILE, contents: INTEL_PROPOSED },
		]);
		expect(await calls(page, 'record_implementation')).toEqual([
			{
				candidateId: 'cand-instruction',
				targetPath: INTEL_FILE,
				expectedHash: sha256(INTEL_PROPOSED),
			},
		]);
		const names = await callNames(page);
		expect(names.indexOf('write_file')).toBeLessThan(names.indexOf('record_implementation'));

		// Implemented is not effective: it says unverified, and invites a check.
		const saved = page.getByTestId('candidate-cand-instruction');
		await expect(saved.getByTestId('candidate-status')).toHaveText('implemented · unverified');
		await expect(saved).toHaveAttribute('data-status', 'implemented');
		await expect(page.getByTestId('intelligence-in-use')).toBeVisible();
		await expect(saved.getByTestId('implemented-note')).toContainText('nobody has checked yet');
		await expect(saved.getByTestId('verification-form')).toBeVisible();
		await expect(saved.getByTestId('candidate-timeline')).toContainText('Saved to AGENTS.md');
		expect(
			await page.evaluate((p) => window.__FACTORAI_TEST__?.files?.[p]?.contents, INTEL_FILE),
		).toBe(INTEL_PROPOSED);
	});

	test('@smoke a failed write records nothing and says the file is as it was', async ({ page }) => {
		// No `files`: the mock's write_file rejects, as a read-only directory would.
		await openTab(page, {
			...withResults([candidateFixture()], {
				prepared: { 'cand-instruction': preparedFixture() },
			}),
			files: undefined,
		});
		const card = await expand(page, 'cand-instruction');
		await card.getByTestId('prepare-change').click();
		await card.getByTestId('save-directly').click();
		await page.getByTestId('intelligence-confirm-save').click();

		await expect(page.getByTestId('intelligence-save-error')).toContainText('Nothing was changed');
		// The dialog stays so it can be tried again; nothing was recorded.
		await expect(page.getByTestId('intelligence-save-dialog')).toBeVisible();
		expect(await calls(page, 'record_implementation')).toHaveLength(0);
		await expect(card).toHaveAttribute('data-status', 'proposed');
	});

	test('@smoke a file that does not exist yet can only be created from the dialog', async ({
		page,
	}) => {
		await openTab(
			page,
			withResults([candidateFixture()], {
				prepared: {
					'cand-instruction': preparedFixture({
						exists: false,
						createdFile: true,
						baseHash: null,
						proposedContent: '- Use pnpm, not npm.\n',
						proposedHash: sha256('- Use pnpm, not npm.\n'),
					}),
				},
			}),
		);
		const card = await expand(page, 'cand-instruction');
		await card.getByTestId('prepare-change').click();
		const prepared = card.getByTestId('prepare-instruction');
		await expect(prepared).toContainText('This file does not exist yet');
		// The viewer cannot open a file that is not there.
		await expect(prepared.getByTestId('open-as-draft')).toHaveCount(0);
		await expect(prepared.getByTestId('draft-blocked')).toContainText('does not exist yet');
		await prepared.getByTestId('save-directly').click();
		await page.getByTestId('intelligence-confirm-save').click();
		expect((await calls(page, 'write_file'))[0]).toMatchObject({
			contents: '- Use pnpm, not npm.\n',
		});
		await expect(card).toHaveAttribute('data-status', 'implemented');
	});

	test('@smoke an instruction already in the file is a duplicate, and nothing is prepared unless asked', async ({
		page,
	}) => {
		await openTab(
			page,
			withResults([candidateFixture()], {
				prepared: {
					'cand-instruction': preparedFixture({
						status: 'duplicate',
						proposedContent: null,
						proposedHash: null,
						diff: null,
						appendedBlock: null,
						duplicateOf: { path: 'AGENTS.md', line: 3, text: 'Use pnpm for everything.' },
					}),
				},
			}),
		);
		const card = await expand(page, 'cand-instruction');
		await card.getByTestId('prepare-change').click();
		const duplicate = card.getByTestId('prepare-duplicate');
		await expect(duplicate).toContainText('already covered');
		await expect(duplicate).toContainText('AGENTS.md:3');
		await expect(duplicate).toContainText('Use pnpm for everything.');
		// No patch, so no way to save one.
		await expect(card.getByTestId('save-directly')).toHaveCount(0);
		await duplicate.getByTestId('prepare-anyway').click();
		await expect
			.poll(async () => (await calls(page, 'prepare_instruction_change')).at(-1))
			.toEqual({ candidateId: 'cand-instruction', allowDuplicate: true });
	});

	test('@smoke the draft goes to the editor, and the editor’s Save is what is recorded', async ({
		page,
	}) => {
		await openTab(
			page,
			withResults([candidateFixture()], {
				prepared: { 'cand-instruction': preparedFixture() },
			}),
		);
		const card = await expand(page, 'cand-instruction');
		await card.getByTestId('prepare-change').click();
		await card.getByTestId('open-as-draft').click();

		// The viewer opened the file, on the changed line, and it is unsaved.
		await expect(page).toHaveURL(/file=%2Fhome%2Falice%2Fcode%2Ffoo%2FAGENTS\.md/);
		await expect(page).toHaveURL(/line=4/);
		await expect(page).toHaveURL(/tab=intelligence/);
		const viewer = page.getByTestId('file-viewer');
		await expect(viewer.getByTestId('file-view-editor')).toBeVisible();
		await expect(viewer.getByTestId('file-view-editor').locator('.view-lines')).toContainText(
			'Use pnpm, not npm.',
		);
		await expect(viewer.getByTestId('viewer-save')).toBeEnabled();
		await expect(card.getByTestId('prepare-waiting')).toContainText('press Save there');
		// Nothing written yet, nothing recorded: opening a draft is not a Save.
		expect(await calls(page, 'write_file')).toHaveLength(0);
		expect(await calls(page, 'record_implementation')).toHaveLength(0);
		await expect(card).toHaveAttribute('data-status', 'proposed');

		await viewer.getByTestId('viewer-save').click();

		expect(await calls(page, 'write_file')).toEqual([
			{ path: INTEL_FILE, contents: INTEL_PROPOSED },
		]);
		await expect
			.poll(async () => calls(page, 'record_implementation'))
			.toEqual([
				{
					candidateId: 'cand-instruction',
					targetPath: INTEL_FILE,
					expectedHash: sha256(INTEL_PROPOSED),
				},
			]);
		await expect(page.getByTestId('candidate-cand-instruction')).toHaveAttribute(
			'data-status',
			'implemented',
		);
		await expect(
			page.getByTestId('candidate-cand-instruction').getByTestId('candidate-status'),
		).toHaveText('implemented · unverified');
	});

	test('@smoke an edited draft is recorded with the hash of what was actually saved', async ({
		page,
	}) => {
		await openTab(
			page,
			withResults([candidateFixture()], {
				prepared: { 'cand-instruction': preparedFixture() },
			}),
		);
		const card = await expand(page, 'cand-instruction');
		await card.getByTestId('prepare-change').click();
		await card.getByTestId('open-as-draft').click();
		const viewer = page.getByTestId('file-viewer');
		const host = viewer.getByTestId('file-view-editor');
		await host.click();
		await expect(host.locator('.monaco-editor').first()).toHaveClass(/(^|\s)focused(\s|$)/);
		await page.keyboard.insertText('# edited ');
		await viewer.getByTestId('viewer-save').click();

		const [write] = await calls(page, 'write_file');
		expect(String(write?.contents)).toContain('Use pnpm, not npm.');
		expect(String(write?.contents)).toContain('# edited ');
		await expect
			.poll(async () => (await calls(page, 'record_implementation')).at(0)?.expectedHash)
			.toBe(sha256(String(write?.contents)));
		// Not the proposal's hash: that is not what is on disk.
		expect(sha256(String(write?.contents))).not.toBe(sha256(INTEL_PROPOSED));
	});

	test('@smoke a person documents a check; it never becomes "effective"', async ({ page }) => {
		await openTab(page, withResults([implemented()]));
		const card = await expand(page, 'cand-instruction');
		await expect(card.getByTestId('candidate-status')).toHaveText('implemented · unverified');

		const form = card.getByTestId('verification-form');
		// Nothing is preselected, so a result cannot be recorded by accident.
		await expect(form.getByTestId('verify-submit')).toBeDisabled();
		await form.getByTestId('verify-method').fill('ran the install steps in a new session');
		await expect(form.getByTestId('verify-submit')).toBeDisabled();
		await form.getByTestId('verify-outcome').click();
		await page.getByTestId('verify-outcome-passed').click();
		await form.getByTestId('verify-note').fill('it reached for pnpm');
		await form.getByTestId('verify-submit').click();

		expect(await calls(page, 'record_verification')).toEqual([
			{
				input: {
					candidateId: 'cand-instruction',
					method: 'ran the install steps in a new session',
					outcome: 'passed',
					note: 'it reached for pnpm',
				},
			},
		]);
		// The status does not move; the chip says what the person recorded.
		await expect(card).toHaveAttribute('data-status', 'implemented');
		await expect(card.getByTestId('candidate-status')).toHaveText('implemented · check passed');
		await expect(card.getByTestId('implemented-note')).toContainText(
			'your word, not a measurement',
		);
		await expect(card.getByTestId('candidate-timeline')).toContainText(
			'Checked by ran the install steps in a new session: passed',
		);
		await expect(card.getByTestId('candidate-timeline')).toContainText('recorded by you');
		await expect(card).not.toContainText('effective');
		// The form is empty again for the next check.
		await expect(form.getByTestId('verify-method')).toHaveValue('');
	});

	test('@smoke retaining asks, takes a reason, and is not a claim that it works', async ({
		page,
	}) => {
		await openTab(page, withResults([implemented()]));
		const card = await expand(page, 'cand-instruction');
		await card.getByTestId('retain').click();
		const confirm = card.getByTestId('retain-confirm');
		await confirm.getByLabel('Reason (optional)').fill('worth keeping');
		await confirm.getByTestId('retain-confirm-ok').click();

		expect(await calls(page, 'mark_retained')).toEqual([
			{ candidateId: 'cand-instruction', reason: 'worth keeping' },
		]);
		await expect(card).toHaveAttribute('data-status', 'retained');
		await expect(card.getByTestId('candidate-status')).toHaveText('retained · unverified');
		await expect(card.getByTestId('implemented-note')).toContainText('not evidence that it works');
		// A kept change can still be taken back.
		await expect(card.getByTestId('revert')).toBeVisible();
	});

	test('@smoke a revert whose file moved shows what to do by hand and writes nothing', async ({
		page,
	}) => {
		await openTab(
			page,
			withResults([implemented()], {
				revertPrepared: {
					'cand-instruction': preparedFixture({
						kind: 'revert',
						status: 'conflict',
						proposedContent: null,
						proposedHash: null,
						diff: null,
						appendedBlock: null,
						conflict: {
							reason: 'AGENTS.md was edited after the instruction was saved.',
							currentHash: 'c'.repeat(64),
							recordedHash: 'd'.repeat(64),
							manualInstructions:
								'Open AGENTS.md and delete the line "- Use pnpm, not npm." Then check again.',
						},
					}),
				},
			}),
		);
		const card = await expand(page, 'cand-instruction');
		await card.getByTestId('revert').click();

		const conflict = card.getByTestId('prepare-conflict');
		await expect(conflict).toContainText('has changed since the instruction was saved');
		await expect(conflict.getByTestId('manual-instructions')).toContainText(
			'delete the line "- Use pnpm, not npm."',
		);
		await expect(conflict).toContainText('nothing was written');
		// There is no Save to press: the app will not overwrite the file.
		await expect(card.getByTestId('save-directly')).toHaveCount(0);
		await expect(card.getByTestId('open-as-draft')).toHaveCount(0);
		const names = await callNames(page);
		expect(names).not.toContain('write_file');
		expect(names).not.toContain('record_revert');
		await expect(card).toHaveAttribute('data-status', 'implemented');

		// Once the person has removed the line by hand, the app checks the file and
		// records the revert - it did not need a patch to do it.
		await conflict.getByTestId('check-removed').click();
		await expect
			.poll(async () => callNames(page))
			.toEqual(expect.arrayContaining(['read_file', 'record_revert']));
		expect((await calls(page, 'record_revert'))[0]).toEqual({
			candidateId: 'cand-instruction',
			targetPath: INTEL_FILE,
			expectedHash: sha256(INTEL_BASE),
		});
		expect(await calls(page, 'write_file')).toHaveLength(0);
		await expect(page.getByTestId('intelligence-closed-toggle')).toHaveText('Show 1 closed');
	});

	test('@smoke a revert that can be proposed is a diff and a Save, then recorded', async ({
		page,
	}) => {
		const revertDiff = [
			'--- a/AGENTS.md',
			'+++ b/AGENTS.md',
			'@@ -1,5 +1,3 @@',
			' # Project',
			' ',
			' Build with `pnpm build`.',
			'-',
			'--- Use pnpm, not npm.',
			'',
		].join('\n');
		await openTab(page, {
			...withResults([implemented()], {
				revertPrepared: {
					'cand-instruction': preparedFixture({
						kind: 'revert',
						proposedContent: INTEL_BASE,
						proposedHash: sha256(INTEL_BASE),
						baseHash: sha256(INTEL_PROPOSED),
						diff: revertDiff,
					}),
				},
			}),
			files: {
				[INTEL_FILE]: {
					path: INTEL_FILE,
					contents: INTEL_PROPOSED,
					size: INTEL_PROPOSED.length,
					isBinary: false,
					truncated: false,
					lineCount: 5,
					lossy: false,
					sopsEncrypted: false,
				},
			},
		});
		const card = await expand(page, 'cand-instruction');
		await card.getByTestId('revert').click();

		const revert = card.getByTestId('prepare-revert');
		await expect(revert).toContainText('Review the revert of AGENTS.md');
		// A removed line that itself starts with dashes is still a removal.
		await expect(revert.getByTestId('intelligence-diff')).toContainText('−2');
		expect(await calls(page, 'write_file')).toHaveLength(0);

		await revert.getByTestId('save-directly').click();
		await expect(page.getByTestId('intelligence-save-dialog')).toContainText(
			'Save the revert to AGENTS.md?',
		);
		await page.getByTestId('intelligence-confirm-save').click();

		expect(await calls(page, 'write_file')).toEqual([{ path: INTEL_FILE, contents: INTEL_BASE }]);
		expect(await calls(page, 'record_revert')).toEqual([
			{
				candidateId: 'cand-instruction',
				targetPath: INTEL_FILE,
				expectedHash: sha256(INTEL_BASE),
			},
		]);
		await expect(page.getByTestId('intelligence-closed-toggle')).toHaveText('Show 1 closed');
	});

	test('@smoke the diff fixture is a real unified diff', async () => {
		// Guards the fixtures the specs above lean on: if the helper stopped
		// producing a hunk, the "changed line" they open the editor on would be
		// wrong without any spec failing for the right reason.
		expect(INTEL_DIFF).toContain('@@ -1,3 +1,5 @@');
		expect(INTEL_PROPOSED.startsWith(INTEL_BASE)).toBe(true);
	});
});
