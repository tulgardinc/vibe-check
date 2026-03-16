import { Command } from 'commander';
import { registerIndexCommand } from './commands/index-cmd.js';
import { registerQueryCommand } from './commands/query-cmd.js';
import { registerStatusCommand } from './commands/status-cmd.js';

const program = new Command();

program
  .name('codeuse')
  .description('Semantic code reuse enforcement for LLM-assisted development')
  .version('0.1.0');

registerIndexCommand(program);
registerQueryCommand(program);
registerStatusCommand(program);

program.parse();
