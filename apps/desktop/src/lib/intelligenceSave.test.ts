import type { FileContents, ImprovementCandidate, PreparedChange } from '@factorai/types';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { emitFileSaved, onFileSaved } from './fileSaved';
import {
	draftBlock,
	expectedHash,
	type Handoff,
	handoffOf,
	recordFromDisk,
	recordSaved,
	saveDirectly,
	sha256Hex,
} from './intelligenceSave';

/** The browser-only bridge reads its fixture off `window`; Node has none. */
function installFixture(fixture: object): {
	calls: Array<{ name: string; args?: Record<string, unknown> }>;
} {
	const calls: Array<{ name: string; args?: Record<string, unknown> }> = [];
	Object.defineProperty(globalThis, 'window', {
		value: { __FACTORAI_TEST__: fixture, __FACTORAI_TEST_CALLS__: calls },
		configurable: true,
		writable: true,
	});
	return { calls };
}

afterEach(() => {
	Reflect.deleteProperty(globalThis, 'window');
});

const TARGET = '/p/AGENTS.md';

const BASE = '# Project\n';
const PROPOSED = '# Project\n\n- Use pnpm, not npm.\n';

function file(contents: string, over: Partial<FileContents> = {}): FileContents {
	return {
		path: TARGET,
		contents,
		size: contents.length,
		isBinary: false,
		truncated: false,
		lineCount: contents.split('\n').length - 1,
		lossy: false,
		sopsEncrypted: false,
		...over,
	};
}

function prepared(over: Partial<PreparedChange> = {}): PreparedChange {
	return {
		candidateId: 'c1',
		kind: 'instruction',
		status: 'ready',
		targetPath: TARGET,
		relativePath: 'AGENTS.md',
		exists: true,
		createdFile: false,
		baseHash: 'b'.repeat(64),
		proposedContent: PROPOSED,
		proposedHash: 'a'.repeat(64),
		appendedBlock: '\n- Use pnpm, not npm.\n',
		diff: '--- a\n+++ b\n@@ -1 +1,3 @@\n # Project\n+\n+- Use pnpm, not npm.\n',
		note: null,
		duplicateOf: null,
		conflict: null,
		...over,
	};
}

function candidate(over: Partial<ImprovementCandidate> = {}): ImprovementCandidate {
	return {
		id: 'c1',
		projectId: 'p1',
		latestRunId: null,
		problemKind: 'project_convention',
		actionKind: 'instruction',
		title: 't',
		problem: 'p',
		recommendation: 'Use pnpm, not npm.',
		rationale: 'r',
		limitations: null,
		status: 'proposed',
		hypothesis: false,
		downgradeNote: null,
		followUp: false,
		independentSessions: 2,
		duplicate: null,
		evidence: [],
		events: [],
		implementation: null,
		createdAt: 1,
		updatedAt: 1,
		...over,
	};
}

function handoff(over: Partial<Handoff> = {}): Handoff {
	const h = handoffOf(prepared(), 'p1');
	if (!h) throw new Error('fixture is not ready');
	return { ...h, ...over };
}

describe('sha256Hex', () => {
	it('matches the published vectors, as lowercase hex over UTF-8', async () => {
		expect(await sha256Hex('')).toBe(
			'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855',
		);
		expect(await sha256Hex('abc')).toBe(
			'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad',
		);
		// Multibyte text is hashed as its UTF-8 bytes, which is what is on disk.
		expect(await sha256Hex('é')).toBe(
			'4a99557e4033c3539de2eb65472017cad5f9557f7a0625a09f1c3f6e2ba69c4c',
		);
	});

	it('says so, rather than hashing wrongly, where the webview has no SubtleCrypto', async () => {
		vi.stubGlobal('crypto', {});
		try {
			await expect(sha256Hex('x')).rejects.toThrow(/SHA-256/);
		} finally {
			vi.unstubAllGlobals();
		}
	});
});

describe('handoffOf', () => {
	it('is a handoff only for a change that carries a patch', () => {
		expect(handoffOf(prepared(), 'p1')).toMatchObject({
			candidateId: 'c1',
			projectId: 'p1',
			kind: 'instruction',
			targetPath: TARGET,
			proposedHash: 'a'.repeat(64),
		});
		expect(
			handoffOf(prepared({ status: 'duplicate', proposedContent: null, proposedHash: null }), 'p1'),
		).toBeNull();
		expect(
			handoffOf(prepared({ status: 'conflict', proposedContent: null, proposedHash: null }), 'p1'),
		).toBeNull();
		// A "ready" change without content is a contract break, not a Save.
		expect(handoffOf(prepared({ proposedContent: null }), 'p1')).toBeNull();
	});
});

describe('expectedHash', () => {
	it('reports the backend’s own hash when the proposal was saved unchanged', async () => {
		expect(await expectedHash(handoff(), PROPOSED)).toBe('a'.repeat(64));
	});

	it('hashes what was actually saved when the person edited the draft first', async () => {
		const edited = `${PROPOSED}- Another line.\n`;
		expect(await expectedHash(handoff(), edited)).toBe(await sha256Hex(edited));
		expect(await expectedHash(handoff(), edited)).not.toBe('a'.repeat(64));
	});
});

describe('recording a Save', () => {
	it('reports an instruction with the saved hash to record_implementation', async () => {
		const { calls } = installFixture({
			settings: { projectIntelligence: 'on' },
			intelligence: { improvements: { p1: [candidate()] }, prepared: { c1: prepared() } },
		});
		const saved = await recordSaved(handoff(), PROPOSED);
		expect(saved.status).toBe('implemented');
		const call = calls.find((c) => c.name === 'record_implementation');
		expect(call?.args).toEqual({
			candidateId: 'c1',
			targetPath: TARGET,
			expectedHash: 'a'.repeat(64),
		});
	});

	it('reports a revert to record_revert, and never to record_implementation', async () => {
		const { calls } = installFixture({
			settings: { projectIntelligence: 'on' },
			intelligence: { improvements: { p1: [candidate({ status: 'implemented' })] } },
		});
		const saved = await recordSaved(handoff({ kind: 'revert' }), BASE);
		expect(saved.status).toBe('reverted');
		expect(calls.map((c) => c.name)).toEqual(['record_revert']);
		expect(calls[0]?.args?.expectedHash).toBe(await sha256Hex(BASE));
	});
});

describe('saveDirectly', () => {
	it('writes the proposal with write_file, then records it, in that order', async () => {
		const { calls } = installFixture({
			settings: { projectIntelligence: 'on' },
			files: { [TARGET]: file(BASE) },
			intelligence: { improvements: { p1: [candidate()] } },
		});
		const outcome = await saveDirectly(handoff());
		expect(outcome.state).toBe('recorded');
		expect(calls.map((c) => c.name)).toEqual(['write_file', 'record_implementation']);
		expect(calls[0]?.args).toEqual({ path: TARGET, contents: PROPOSED });
	});

	it('records nothing when the write failed, and says the file is as it was', async () => {
		// No `files` in the fixture: the mock's write_file rejects.
		const { calls } = installFixture({
			settings: { projectIntelligence: 'on' },
			intelligence: { improvements: { p1: [candidate()] } },
		});
		const outcome = await saveDirectly(handoff());
		expect(outcome.state).toBe('write-failed');
		expect(calls.map((c) => c.name)).toEqual(['write_file']);
	});

	it('keeps a write that the backend then refused distinct from one that never happened', async () => {
		const { calls } = installFixture({
			settings: { projectIntelligence: 'on' },
			files: { [TARGET]: file(BASE) },
			// The candidate does not exist: record_implementation rejects.
			intelligence: { improvements: { p1: [] } },
		});
		const outcome = await saveDirectly(handoff());
		expect(outcome.state).toBe('record-failed');
		expect(calls.map((c) => c.name)).toEqual(['write_file', 'record_implementation']);
	});
});

describe('recordFromDisk', () => {
	it('reads the file and reports the hash of what it holds', async () => {
		const { calls } = installFixture({
			settings: { projectIntelligence: 'on' },
			files: { [TARGET]: file(`${PROPOSED}- Extra.\n`) },
			intelligence: { improvements: { p1: [candidate()] } },
		});
		const outcome = await recordFromDisk(handoff());
		expect(outcome.state).toBe('recorded');
		expect(calls.map((c) => c.name)).toEqual(['read_file', 'record_implementation']);
		expect(calls[1]?.args?.expectedHash).toBe(await sha256Hex(`${PROPOSED}- Extra.\n`));
	});

	it('will not vouch for text it only half read', async () => {
		installFixture({
			settings: { projectIntelligence: 'on' },
			files: { [TARGET]: file(BASE, { truncated: true }) },
			intelligence: { improvements: { p1: [candidate()] } },
		});
		expect((await recordFromDisk(handoff())).state).toBe('record-failed');
	});

	it('reports a file that is not there as a failed check, not a crash', async () => {
		installFixture({ settings: { projectIntelligence: 'on' }, files: {} });
		expect((await recordFromDisk(handoff())).state).toBe('record-failed');
	});
});

describe('draftBlock', () => {
	const base = { exists: true, targetPath: TARGET, viewerPath: null, existingDraft: undefined };

	it('allows a draft for a file that exists and is not being edited', () => {
		expect(draftBlock(base)).toBeNull();
		expect(draftBlock({ ...base, viewerPath: '/p/other.ts' })).toBeNull();
	});

	it('cannot open a file that is not there, so it is the confirm dialog’s case', () => {
		expect(draftBlock({ ...base, exists: false })).toBe('new-file');
	});

	it('does not seed a draft under an editor that has already read its buffer', () => {
		expect(draftBlock({ ...base, viewerPath: TARGET })).toBe('open-in-viewer');
	});

	it('does not replace unsaved edits a person already has', () => {
		expect(draftBlock({ ...base, existingDraft: 'mine' })).toBe('has-draft');
		// An empty draft is still a draft.
		expect(draftBlock({ ...base, existingDraft: '' })).toBe('has-draft');
	});
});

describe('fileSaved', () => {
	it('tells every listener what was written, and stops when asked', () => {
		const seen: string[] = [];
		const stop = onFileSaved((e) => seen.push(`${e.path}=${e.contents}`));
		emitFileSaved({ path: '/a', contents: 'one' });
		stop();
		emitFileSaved({ path: '/a', contents: 'two' });
		expect(seen).toEqual(['/a=one']);
	});

	it('does not let one listener’s failure reach the save or the next listener', () => {
		const seen: string[] = [];
		const stopBad = onFileSaved(() => {
			throw new Error('listener bug');
		});
		const stopGood = onFileSaved((e) => seen.push(e.contents));
		expect(() => emitFileSaved({ path: '/a', contents: 'x' })).not.toThrow();
		expect(seen).toEqual(['x']);
		stopBad();
		stopGood();
	});
});
