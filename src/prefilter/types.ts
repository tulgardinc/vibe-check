export interface PrefilterMatch {
  queryFunction: string;
  matchedFunction: string;
  matchedFilePath: string;
  matchedStartLine: number;
  cloneType: 1 | 2;
  tool: 'jscpd';
  confidence: number;
}
