import type { ImprovementCandidate } from '@factorai/types';
import { afterEach, describe, expect, it } from 'vitest';
import { intelligenceErrorCode } from './intelligence';
import { cmd } from './tauri';

/** The renderer's browser-only bridge reads its data off `window`. Node has
 *  none, so a test installs one and removes it again. */
function installFixture(fixture: object | undefined): void {
	Object.defineProperty(globalThis, 'window', {
		value: fixture ? { __FACTORAI_TEST__: fixture } : {},
		configurable: true,
		writable: true,
	});
}

afterEach(() => {
	Reflect.deleteProperty(globalThis, 'window');
});

function candidate(id: string): ImprovementCandidate {
	return {
		id,
		projectId: 'p1',
		latestRunId: null,
		problemKind: 'project_convention',
		actionKind: 'instruction',
		title: 'Synthetic fixture candidate',
		problem: 'synthetic',
		recommendation: 'Use pnpm',
		rationale: 'synthetic',
		limitations: null,
		status: 'proposed',
		hypothesis: true,
		downgradeNote: null,
		followUp: false,
		independentSessions: 1,
		duplicate: null,
		evidence: [],
		events: [],
		implementation: null,
		createdAt: 1,
		updatedAt: 1,
	};
}

describe('the browser-only Project Intelligence bridge (F33)', () => {
	it('is off with no fixture, and every command refuses the way the real ones do', async () => {
		installFixture(undefined);
		expect(await cmd.getIntelligenceStatus()).toEqual({ enabled: false, provider: null });
		for (const call of [
			() => cmd.previewAnalysis('p1'),
			() =>
				cmd.startAnalysis({
					projectId: 'p1',
					consentRemote: true,
					previewHash: 'h',
				}),
			() => cmd.listImprovements('p1'),
			() => cmd.latestIntelligenceRun('p1'),
			() => cmd.dismissImprovement('c1'),
		]) {
			const error = await call().then(
				() => null,
				(e: unknown) => e,
			);
			expect(intelligenceErrorCode(error)).toBe('disabled');
		}
		await expect(cmd.cancelAnalysis('r1')).resolves.toBeUndefined();
	});

	it('follows the same setting the real switch is', async () => {
		const settings: Record<string, string> = { projectIntelligence: 'on' };
		installFixture({ settings });
		const status = await cmd.getIntelligenceStatus();
		expect(status.enabled).toBe(true);
		expect(status.provider?.remote).toBe(true);
		await cmd.setSetting('projectIntelligence', null);
		expect((await cmd.getIntelligenceStatus()).enabled).toBe(false);
	});

	it('answers empty until a fixture says otherwise', async () => {
		installFixture({ settings: { projectIntelligence: 'on' } });
		expect(await cmd.listImprovements('p1')).toEqual([]);
		expect(await cmd.latestIntelligenceRun('p1')).toBeNull();
		const preview = await cmd.previewAnalysis('p1');
		expect(preview).toMatchObject({ projectId: 'p1', sessionCount: 0, signals: [] });
		expect(
			(await cmd.startAnalysis({ projectId: 'p1', consentRemote: false, previewHash: 'h' })).runId,
		).toBe('mock-run');
	});

	it('applies the candidate commands to the fixture so a refetch shows them', async () => {
		const c = candidate('c1');
		installFixture({
			settings: { projectIntelligence: 'on' },
			intelligence: { improvements: { p1: [c] } },
		});
		const followed = await cmd.markFollowUp('c1', true, 'later');
		expect(followed.followUp).toBe(true);
		const implemented = await cmd.recordImplementation('c1', '/p/AGENTS.md', 'a'.repeat(64));
		expect(implemented.status).toBe('implemented');
		expect(implemented.implementation?.afterHash).toBe('a'.repeat(64));
		await cmd.recordVerification({ candidateId: 'c1', method: 'looked', outcome: 'passed' });
		const [after] = await cmd.listImprovements('p1');
		expect(after?.status).toBe('implemented');
		const kinds = after?.events.map((e) => e.eventType);
		expect(kinds).toEqual(['follow_up', 'implemented', 'verification']);
		const ids = after?.events.map((e) => e.id) ?? [];
		expect(ids).toEqual([...ids].sort((a, b) => a - b));
		await expect(cmd.dismissImprovement('nope')).rejects.toMatchObject({ kind: 'Intelligence' });
	});
});
