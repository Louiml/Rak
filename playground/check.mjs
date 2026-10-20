// Runs a Rak program in the playground's WebAssembly module, the way the
// browser page does.
//
// The page loads `docs/rak.wasm`, hands the module one import -- the browser's
// random bytes -- writes the program into the module's memory, runs it, and
// reads the output back out of the same memory. This script does exactly
// that, so CI can check the artifact the site actually serves rather than
// only checking that it compiles.
//
//   node playground/check.mjs <module.wasm> [program.rak]
//
// With no program, it runs a small one of its own. It exits non-zero if the
// module does not load, if the program fails, or if the output is not what
// was expected.

import { readFileSync } from 'node:fs';
import { randomFillSync } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));

const modulePath = process.argv[2];
if (!modulePath) {
  console.error('usage: node playground/check.mjs <module.wasm> [program.rak]');
  process.exit(2);
}

/// The program the check runs when none is given.
///
/// It is deliberately dull: an arithmetic expression, a string, and a `dump`.
/// A module that cannot do this cannot do anything.
const DEFAULT_PROGRAM = 'let mut total = 0\nfor i in 1..10 {\n  total = total + i\n}\ndump total\ndump "rak in the browser"\n';

/// What the program above is expected to print.
const EXPECTED = ['[DUMP] 55', '[DUMP] rak in the browser'];

const program = process.argv[3]
  ? readFileSync(process.argv[3], 'utf8')
  : DEFAULT_PROGRAM;

const bytes = readFileSync(modulePath);

// The module's memory is not known until it is instantiated, but the import
// may be called during instantiation, so the closure reads it from here.
let memory = null;

const instance = await WebAssembly.instantiate(bytes, {
  rak: {
    random_bytes(ptr, len) {
      try {
        randomFillSync(new Uint8Array(memory.buffer, ptr, len));
        return 0;
      } catch {
        return 1;
      }
    },
  },
});

const exports = instance.instance.exports;
memory = exports.memory;

const source = new TextEncoder().encode(program);
new Uint8Array(memory.buffer, exports.program_ptr() >>> 0, source.length).set(source);

const outLen = exports.run(0, source.length) >>> 0;
// The memory may have grown, which detaches the previous view, so the output
// is read from the buffer as it is now.
const output = new TextDecoder().decode(
  new Uint8Array(memory.buffer, exports.out_ptr() >>> 0, outLen),
);

console.log(`module:  ${modulePath}`);
console.log(`program: ${program.replace(/\n/g, ' | ')}`);
console.log(`output:\n${output}`);

if (!output.includes('exit 0')) {
  console.error('the program did not finish successfully');
  process.exit(1);
}

if (process.argv[3] === undefined) {
  const missing = EXPECTED.filter((line) => !output.includes(line));
  if (missing.length > 0) {
    console.error(`the output is missing: ${missing.join(', ')}`);
    process.exit(1);
  }
}

console.log('ok');
