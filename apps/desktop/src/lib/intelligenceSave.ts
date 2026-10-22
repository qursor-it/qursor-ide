import type { ImprovementCandidate, PreparedChange } from '@factorai/types';
import { cmd } from '@lib/tauri';

/**
 * The renderer's half of "a project file is written only by the human's Save"
 * (ADR-0084 decision 6, ADR-0039; specs/05-features.md F33 § "Changing an
 * instruction file").
 *
 * The backend prepared a change and wrote nothing. Here it is handed to a Save
 * the person presses - the viewer's, or one confirm dialog's - and what was
 * saved is then **reported** to the backend, which re-reads the file and
 * decides whether to believe it. Nothing in this module writes without being
 * called from a click, and nothing records without a write having happened.
 */

/** sha256 of text as lowercase hex, over its UTF-8 bytes - what the backend
 *  computes over the file's bytes, so the two agree for any text that decoded. */
export async function sha256Hex(text: string): Promise<string> {
	const subtle = globalThis.crypto?.subtle;
	// A webview outside a secure context has no `subtle`. A wrong or missing
	// hash would be reported as a file conflict that is not one, so say what it is.
	if (!subtle) throw new Error('This webview cannot compute a SHA-256 hash.');
	const digest = await subtle.digest('SHA-256', new TextEncoder().encode(text));
	return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, '0')).join('');
}

export type HandoffKind = 'instruction' | 'revert';

/** What recording a Save needs to know. */
export interface RecordTarget {
	candidateId: string;
	projectId: string;
	kind: HandoffKind;
	/** Absolute and canonical, inside the project: what is opened and written. */
	targetPath: string;
	/** The proposal, when there was one, so a Save that changed nothing reports
	 *  the backend's own hash. A change removed by hand has none. */
	proposedContent?: string;
	proposedHash?: string;
}

/** A prepared change that is ready to be saved, with what recording it needs. */
export interface Handoff extends RecordTarget {
	relativePath: string;
	/** `false`: saving creates the file, and the viewer cannot open it yet. */
	exists: boolean;
	proposedContent: string;
	proposedHash: string;
	/** The unified diff the person reviewed. */
	diff: string | null;
}

/** The handoff for a change, or `null` when there is nothing to save: a
 *  duplicate or a conflict carries no patch (F33). */
export function handoffOf(change: PreparedChange, projectId: string): Handoff | null {
	if (change.status !== 'ready') return null;
	if (change.proposedContent === null || change.proposedHash === null) return null;
	return {
		candidateId: change.candidateId,
		projectId,
		kind: change.kind,
		targetPath: change.targetPath,
		relativePath: change.relativePath,
		exists: change.exists,
		proposedContent: change.proposedContent,
		proposedHash: change.proposedHash,
		diff: change.diff,
	};
}

/** A revert the backend could not propose a patch for (`status: conflict`): the
 *  person removes the line by hand, and this is what lets them say so. The
 *  backend accepts that as a revert too (F33), once it finds the line gone. */
export function manualRevertTarget(change: PreparedChange, projectId: string): RecordTarget {
	return {
		candidateId: change.candidateId,
		projectId,
		kind: 'revert',
		targetPath: change.targetPath,
	};
}

/**
 * The hash to report for what was saved: `proposedHash` when the text is exactly
 * the proposal (the backend computed it, so there is no second implementation to
 * disagree with), and the hash of the text otherwise - a person may have edited
 * the draft before pressing Save, and the backend then checks that the prepared
 * line survived.
 */
export async function expectedHash(target: RecordTarget, savedText: string): Promise<string> {
	return target.proposedHash !== undefined && savedText === target.proposedContent
		? target.proposedHash
		: sha256Hex(savedText);
}

/** Report a Save that happened. The backend re-reads the file and refuses with
 *  `not_saved` or `file_conflict` when it does not match. */
export async function recordSaved(
	target: RecordTarget,
	savedText: string,
): Promise<ImprovementCandidate> {
	const hash = await expectedHash(target, savedText);
	return target.kind === 'revert'
		? cmd.recordRevert(target.candidateId, target.targetPath, hash)
		: cmd.recordImplementation(target.candidateId, target.targetPath, hash);
}

/** What happened when a save was attempted and recorded. The three failures are
 *  different facts about the person's file, so they are different states. */
export type SaveOutcome =
	| { state: 'recorded'; candidate: ImprovementCandidate }
	/** `write_file` failed: the file is as it was. */
	| { state: 'write-failed'; error: unknown }
	/** The file was written, and the backend would not record it. */
	| { state: 'record-failed'; error: unknown };

/**
 * Write the proposal with the existing `write_file` and record it.
 *
 * **Only ever called from the confirm dialog's explicit button.** It is the same
 * command the viewer's Save uses (ADR-0039: one write path), so permissions,
 * the atomic rename and the symlink resolution are the viewer's.
 */
export async function saveDirectly(handoff: Handoff): Promise<SaveOutcome> {
	try {
		await cmd.writeFile(handoff.targetPath, handoff.proposedContent);
	} catch (error) {
		return { state: 'write-failed', error };
	}
	try {
		return {
			state: 'recorded',
			candidate: await recordSaved(handoff, handoff.proposedContent),
		};
	} catch (error) {
		return { state: 'record-failed', error };
	}
}

/**
 * Record whatever the file holds now, for a person who saved it somewhere
 * the app did not see (the viewer was closed, or another editor did it). Reads
 * the file, hashes it, and lets the backend decide.
 */
export async function recordFromDisk(
	target: RecordTarget,
): Promise<
	| { state: 'recorded'; candidate: ImprovementCandidate }
	| { state: 'record-failed'; error: unknown }
> {
	try {
		const file = await cmd.readFile(target.targetPath);
		if (file.isBinary || file.truncated || file.lossy) {
			throw new Error('The file cannot be read back as complete text.');
		}
		return { state: 'recorded', candidate: await recordSaved(target, file.contents) };
	} catch (error) {
		return { state: 'record-failed', error };
	}
}

/** Why a change cannot be opened as a draft in the viewer, or `null` when it can. */
export type DraftBlock =
	/** The file does not exist, and the viewer cannot open a file that is not there. */
	| 'new-file'
	/** It is the file the viewer has open: a mounted editor reads its draft only
	 *  once, so seeding one would change nothing on screen. */
	| 'open-in-viewer'
	/** The person already has unsaved edits to it, which a seed would replace. */
	| 'has-draft';

export function draftBlock(args: {
	exists: boolean;
	targetPath: string;
	/** `?file=`: what the viewer is showing. */
	viewerPath: string | null;
	/** Whatever the draft store holds for the target. */
	existingDraft: string | undefined;
}): DraftBlock | null {
	if (!args.exists) return 'new-file';
	if (args.viewerPath === args.targetPath) return 'open-in-viewer';
	if (args.existingDraft !== undefined) return 'has-draft';
	return null;
}
