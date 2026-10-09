import { describe, expect, it } from 'vitest';
import { isWithin, relativeToRoot } from './paths';

describe('relativeToRoot', () => {
	it('strips the root and its separator, with no leading ./', () => {
		expect(relativeToRoot('/home/a/code/foo/src/main.ts', '/home/a/code/foo')).toBe('src/main.ts');
		expect(relativeToRoot('/home/a/code/foo/README.md', '/home/a/code/foo')).toBe('README.md');
	});

	it('calls the root itself .', () => {
		expect(relativeToRoot('/home/a/code/foo', '/home/a/code/foo')).toBe('.');
	});

	it('tolerates a trailing slash on the root', () => {
		expect(relativeToRoot('/home/a/code/foo/src', '/home/a/code/foo/')).toBe('src');
	});

	it('leaves a path outside the root absolute rather than piling up ../', () => {
		// A symlink target elsewhere on disk. Relative would only resolve from one
		// working directory; absolute is true from anywhere.
		expect(relativeToRoot('/etc/hosts', '/home/a/code/foo')).toBe('/etc/hosts');
	});

	it('does not treat a sibling with a shared prefix as inside', () => {
		expect(relativeToRoot('/home/a/code/foobar/x.ts', '/home/a/code/foo')).toBe(
			'/home/a/code/foobar/x.ts',
		);
	});

	it('returns the path unchanged with no root', () => {
		expect(relativeToRoot('/home/a/x.ts', '')).toBe('/home/a/x.ts');
	});

	it('returns the root itself for a POSIX root with a trailing separator', () => {
		expect(relativeToRoot('/home/a/code/foo/', '/home/a/code/foo')).toBe('.');
	});

	it('compares Windows drive paths case-insensitively and keeps the path spelling', () => {
		expect(relativeToRoot('c:\\Users\\Alice\\Repo\\SRC\\main.ts', 'C:\\users\\alice\\repo')).toBe(
			'SRC\\main.ts',
		);
	});

	it('uses the separator style from a Windows path', () => {
		expect(relativeToRoot('C:/Users/Alice/Repo/src/main.ts', 'c:/users/alice/repo')).toBe(
			'src/main.ts',
		);
	});

	it('matches an extended drive path to its ordinary drive root', () => {
		expect(
			relativeToRoot('\\\\?\\C:\\Users\\Alice\\Repo\\src\\main.ts', 'c:\\users\\alice\\repo'),
		).toBe('src\\main.ts');
	});

	it('matches an extended UNC path to its ordinary share root', () => {
		expect(
			relativeToRoot(
				'\\\\?\\UNC\\SERVER\\Share\\Repos\\Factor\\src\\main.ts',
				'\\\\server\\share\\repos\\factor',
			),
		).toBe('src\\main.ts');
	});

	it('leaves a Windows sibling with a shared prefix absolute', () => {
		expect(relativeToRoot('C:\\repo-old\\src\\main.ts', 'c:\\repo')).toBe(
			'C:\\repo-old\\src\\main.ts',
		);
	});

	it('preserves a trailing Windows path separator in the relative result', () => {
		expect(relativeToRoot('C:\\repo\\src\\', 'c:\\repo')).toBe('src\\');
	});

	it('treats Windows roots with different casing as the same path', () => {
		expect(relativeToRoot('C:\\REPO', 'c:\\repo\\')).toBe('.');
	});
});

describe('isWithin', () => {
	it('holds for a file under the root', () => {
		expect(isWithin('/home/a/code/foo/src/main.ts', '/home/a/code/foo')).toBe(true);
	});

	it('holds for the root itself', () => {
		expect(isWithin('/home/a/code/foo', '/home/a/code/foo')).toBe(true);
	});

	it('rejects a sibling with a shared prefix', () => {
		expect(isWithin('/home/a/code/foobar/x.ts', '/home/a/code/foo')).toBe(false);
	});

	it('tolerates a trailing slash on the root', () => {
		expect(isWithin('/home/a/code/foo/src', '/home/a/code/foo/')).toBe(true);
	});

	it('compares Windows drive paths case-insensitively', () => {
		expect(isWithin('c:\\Users\\Alice\\Repo\\src\\main.ts', 'C:\\users\\alice\\repo')).toBe(true);
	});

	it('matches an extended drive path to an ordinary drive root', () => {
		expect(isWithin('\\\\?\\C:\\Users\\Alice\\Repo\\src', 'c:\\users\\alice\\repo')).toBe(true);
	});

	it('matches an extended UNC path to an ordinary UNC root', () => {
		expect(
			isWithin('\\\\?\\UNC\\SERVER\\Share\\Repos\\Factor\\src', '\\\\server\\share\\repos\\factor'),
		).toBe(true);
	});

	it('rejects a Windows sibling that shares a component prefix', () => {
		expect(isWithin('C:\\repo-old\\src', 'C:\\repo')).toBe(false);
	});

	it('rejects a UNC path on a different share', () => {
		expect(isWithin('\\\\server\\share-two\\project\\src', '\\\\server\\share\\project')).toBe(
			false,
		);
	});

	it('keeps POSIX comparisons case-sensitive', () => {
		expect(isWithin('/Home/a/repo/src', '/home/a/repo')).toBe(false);
		expect(relativeToRoot('/Home/a/repo/src', '/home/a/repo')).toBe('/Home/a/repo/src');
	});
});
