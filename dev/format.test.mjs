// Loads the plugin UI in jsdom and exercises the inlined formatter for real: the
// bundle is copied into ui/index.html by dev/inline-prettier.mjs, so this is what
// catches a truncated or stale inline block (jsdom executes the scripts, then we
// format a snippet through the same globals the button uses).
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const here = path.dirname(fileURLToPath(import.meta.url));
const html = readFileSync(path.join(here, '..', 'ui', 'index.html'), 'utf8');
const { JSDOM } = await import('jsdom');
const dom = new JSDOM(html, {
  runScripts: 'dangerously',
  url: 'http://localhost/',
  // The app boots against the host bridge; stub it so the page script runs to
  // completion instead of throwing on `dbxPlugin.ready` (see dev/ui.test.mjs).
  beforeParse(win) {
    win.dbxPlugin = { ready: Promise.resolve(), request: () => Promise.resolve({}), on: () => {} };
  },
});
const { window } = dom;

let failures = 0;
function check(name, condition, detail = '') {
  console.log(`${condition ? 'PASS' : 'FAIL'}  ${name}${detail ? ` -> ${detail}` : ''}`);
  if (!condition) failures += 1;
}

check('prettier global present', typeof window.prettier?.format === 'function');
check('parsers registered', ['estree', 'babel', 'typescript'].every((k) => !!window.prettierPlugins?.[k]));

if (window.prettier && window.prettierPlugins) {
  const cases = [
    ['babel', 'function  add(a,b){return a+b}', 'function add(a, b) {'],
    ['typescript', 'let  x: number =1', 'let x: number = 1;'],
  ];
  for (const [parser, source, needle] of cases) {
    const out = await window.prettier.format(source, {
      parser,
      plugins: Object.values(window.prettierPlugins),
      tabWidth: 2,
      semi: true,
    });
    check(`prettier formats via ${parser}`, out.includes(needle), JSON.stringify(out.split('\n')[0] ?? ''));
  }
}

check('format button exists in the editor toolbar', html.includes('id="formatBtn"'));
check('only js/ts map to a parser', /PRETTIER_PARSER = \{ javascript: "babel", typescript: "typescript" \}/.test(html));

console.log(failures ? `${failures} check(s) failed` : 'RESULT: formatter checks passed');
process.exit(failures ? 1 : 0);
