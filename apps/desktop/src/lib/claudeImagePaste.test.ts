import { describe, expect, it } from 'vitest';
import { claudeImagePasteInput, isImageOnlyPaste, isWindowsPasteChord } from './claudeImagePaste';

function data(text: string, items: Array<{ kind: string; type: string }>) {
	return { getData: (f: string) => (f === 'text/plain' ? text : ''), items };
}

describe('claudeImagePasteInput', () => {
	it('is Alt+V on Windows and Ctrl+V elsewhere', () => {
		expect(claudeImagePasteInput(true)).toBe('\x1bv');
		expect(claudeImagePasteInput(false)).toBe('\x16');
	});
});

describe('isImageOnlyPaste', () => {
	it('is true for an image and no text', () => {
		expect(isImageOnlyPaste(data('', [{ kind: 'file', type: 'image/png' }]))).toBe(true);
		expect(isImageOnlyPaste(data('  \n', [{ kind: 'file', type: 'image/jpeg' }]))).toBe(true);
	});

	it('is false when there is text, whatever else is there', () => {
		expect(
			isImageOnlyPaste(
				data('https://x/y.png', [
					{ kind: 'string', type: 'text/plain' },
					{ kind: 'file', type: 'image/png' },
				]),
			),
		).toBe(false);
	});

	it('is false for no image, a non-image file, or nothing at all', () => {
		expect(isImageOnlyPaste(data('', []))).toBe(false);
		expect(isImageOnlyPaste(data('', [{ kind: 'file', type: 'application/pdf' }]))).toBe(false);
		expect(isImageOnlyPaste(data('', [{ kind: 'string', type: 'image/png' }]))).toBe(false);
	});
});

describe('isWindowsPasteChord', () => {
	const chord = { key: 'v', ctrlKey: true, shiftKey: false, altKey: false, metaKey: false };
	it('matches a bare Ctrl+V', () => {
		expect(isWindowsPasteChord(chord)).toBe(true);
		expect(isWindowsPasteChord({ ...chord, key: 'V' })).toBe(true);
	});
	it('does not match another chord', () => {
		expect(isWindowsPasteChord({ ...chord, shiftKey: true })).toBe(false);
		expect(isWindowsPasteChord({ ...chord, altKey: true })).toBe(false);
		expect(isWindowsPasteChord({ ...chord, ctrlKey: false })).toBe(false);
		expect(isWindowsPasteChord({ ...chord, key: 'c' })).toBe(false);
	});
});
