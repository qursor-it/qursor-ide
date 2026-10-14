import { describe, expect, it, vi } from 'vitest';
import { createWindowRevealer } from '@lib/windowReveal';

describe('createWindowRevealer', () => {
	it('shows the window once, however often it is asked', () => {
		const show = vi.fn(() => Promise.resolve());
		const reveal = createWindowRevealer(show, () => true);
		reveal();
		reveal();
		expect(show).toHaveBeenCalledTimes(1);
	});

	it('does nothing outside a Tauri webview', () => {
		const show = vi.fn(() => Promise.resolve());
		createWindowRevealer(show, () => false)();
		expect(show).not.toHaveBeenCalled();
	});

	it('does not throw when showing fails', async () => {
		const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
		const reveal = createWindowRevealer(
			() => Promise.reject(new Error('denied')),
			() => true,
		);
		expect(() => reveal()).not.toThrow();
		await Promise.resolve();
		await Promise.resolve();
		expect(error).toHaveBeenCalled();
		error.mockRestore();
	});
});
