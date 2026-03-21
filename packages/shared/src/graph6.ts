// graph6 decoder for the browser

/** Decode a graph6 string into a symmetric boolean adjacency matrix. */
export function decodeGraph6(s: string): boolean[][] {
	const bytes = [...s].map(c => c.charCodeAt(0));
	if (bytes.length === 0) return [];

	// Decode N(n)
	const n = bytes[0] - 63;
	if (n === 0) return [];

	const rest = bytes.slice(1);

	// Unpack 6-bit groups into bits
	const bits: boolean[] = [];
	for (const b of rest) {
		const val = b - 63;
		for (let k = 0; k < 6; k++) {
			bits.push(((val >> (5 - k)) & 1) === 1);
		}
	}

	// Build adjacency matrix (column-major upper triangle)
	const matrix: boolean[][] = Array.from({ length: n }, () => Array(n).fill(false));
	let idx = 0;
	for (let j = 1; j < n; j++) {
		for (let i = 0; i < j; i++) {
			if (idx < bits.length && bits[idx]) {
				matrix[i][j] = true;
				matrix[j][i] = true;
			}
			idx++;
		}
	}

	return matrix;
}

/** Count edges in the upper triangle. */
export function edgeCount(matrix: boolean[][]): number {
	let count = 0;
	for (let i = 0; i < matrix.length; i++) {
		for (let j = i + 1; j < matrix.length; j++) {
			if (matrix[i][j]) count++;
		}
	}
	return count;
}
