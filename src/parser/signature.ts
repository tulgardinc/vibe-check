import { sha256 } from '../util/hash.js';
import type { ParamInfo } from './types.js';

/**
 * Compute a stable 8-hex-char signature hash from a function's type contract.
 * Changes when the function name, parameter types, or return type change.
 * Stable across formatting/whitespace differences.
 */
export function computeSignatureHash(
  name: string,
  params: ParamInfo[],
  returnType: string | null,
): string {
  const normalizedName = name.toLowerCase().trim();
  const normalizedParams = params
    .map((p) => (p.type ?? 'any').replace(/\s+/g, '').toLowerCase())
    .join(',');
  const normalizedReturn = (returnType ?? 'void').replace(/\s+/g, '').toLowerCase();

  const input = `${normalizedName}(${normalizedParams}):${normalizedReturn}`;
  return sha256(input).slice(0, 8);
}
