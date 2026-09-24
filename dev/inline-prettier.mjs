// Inline prettier's standalone UMD build + the parsers LeetCode actually offers
// (JavaScript via babel, TypeScript) into ui/index.html, right before the app
// script. The plugin UI is a single self-contained file with no bundler and the
// sandbox may block same-origin script loads, so inlining is the only route that
// is guaranteed to work offline. Marked so re-running replaces the block.
import { readFileSync, writeFileSync } from 'node:fs';

const file = 'ui/index.html';
const vendors = [
  'node_modules/prettier/standalone.js',
  'node_modules/prettier/plugins/estree.js',
  'node_modules/prettier/plugins/babel.js',
  'node_modules/prettier/plugins/typescript.js',
];

const begin = '<!-- formatter:begin -->';
const end = '<!-- formatter:end -->';
const block = [
  begin,
  ...vendors.map((v) => {
    const code = readFileSync(v, 'utf8').replace(/\/\/# sourceMappingURL=.*$/gm, '');
    return `<script>${code}</script>`;
  }),
  end,
].join('\n');

let html = readFileSync(file, 'utf8');
const existing = html.indexOf(begin);
if (existing >= 0) {
  const stop = html.indexOf(end, existing) + end.length;
  html = `${html.slice(0, existing)}${block}${html.slice(stop)}`;
} else {
  const anchor = html.indexOf('<script>\n      const $ = (sel) => document.querySelector(sel);');
  if (anchor < 0) throw new Error('app script anchor not found');
  html = `${html.slice(0, anchor)}${block}\n    ${html.slice(anchor)}`;
}
writeFileSync(file, html);
console.log('inlined', vendors.length, 'vendor files;', Math.round(html.length / 1024), 'KiB total');
