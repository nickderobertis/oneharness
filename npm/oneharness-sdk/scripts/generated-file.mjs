import { existsSync, readFileSync } from "node:fs";

/**
 * @param {string} path
 * @param {Buffer} expected
 */
export function generatedFileMatches(path, expected) {
	return existsSync(path) && readFileSync(path).equals(expected);
}
