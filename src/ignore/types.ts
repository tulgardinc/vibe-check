export interface ExclusionSide {
  path: string;
  function: string;
  signatureHash: string;
}

export interface Exclusion {
  reason: string;
  added: string;
  pair: {
    a: ExclusionSide;
    b: ExclusionSide;
  };
}

export interface IgnoreFile {
  version: 1;
  exclusions: Exclusion[];
}

export interface StaleWarning {
  exclusionIndex: number;
  side: 'a' | 'b';
  functionName: string;
  path: string;
  reason: string;
}
