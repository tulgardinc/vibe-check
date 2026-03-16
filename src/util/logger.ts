export type LogLevel = 'quiet' | 'normal' | 'verbose';

let currentLevel: LogLevel = 'normal';

export function setLogLevel(level: LogLevel): void {
  currentLevel = level;
}

export function info(message: string): void {
  if (currentLevel !== 'quiet') {
    console.log(message);
  }
}

export function verbose(message: string): void {
  if (currentLevel === 'verbose') {
    console.log(message);
  }
}

export function warn(message: string): void {
  console.error(`warn: ${message}`);
}

export function error(message: string): void {
  console.error(`error: ${message}`);
}
