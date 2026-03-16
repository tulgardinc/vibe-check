export type LogLevel = 'quiet' | 'normal' | 'verbose';

let currentLevel: LogLevel = 'normal';

const useColor =
  !process.env['NO_COLOR'] &&
  process.stderr.isTTY === true;

const reset = useColor ? '\x1b[0m' : '';
const bold = useColor ? '\x1b[1m' : '';
const dim = useColor ? '\x1b[2m' : '';
const red = useColor ? '\x1b[31m' : '';
const yellow = useColor ? '\x1b[33m' : '';
const green = useColor ? '\x1b[32m' : '';
const cyan = useColor ? '\x1b[36m' : '';

export function setLogLevel(level: LogLevel): void {
  currentLevel = level;
}

export function info(message: string): void {
  if (currentLevel !== 'quiet') {
    console.log(`${cyan}info${reset}  ${message}`);
  }
}

export function success(message: string): void {
  if (currentLevel !== 'quiet') {
    console.log(`${green}${bold}done${reset}  ${message}`);
  }
}

export function verbose(message: string): void {
  if (currentLevel === 'verbose') {
    console.log(`${dim}    ${message}${reset}`);
  }
}

export function warn(message: string): void {
  console.error(`${yellow}${bold}warn${reset}  ${message}`);
}

export function error(message: string): void {
  console.error(`${red}${bold}error${reset} ${message}`);
}
