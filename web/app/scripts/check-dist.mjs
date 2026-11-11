// The server sends `script-src 'self'; style-src 'self'` with every page, so a build that
// inlines a script, a style sheet or a style attribute would ship pages the browser blocks.
// This fails the build instead.
import { readdir, readFile } from 'node:fs/promises';
import { join } from 'node:path';

const dist = new URL('../dist/', import.meta.url).pathname;
const problems = [];

async function* pages(dir) {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) yield* pages(path);
    else if (entry.name.endsWith('.html')) yield path;
  }
}

for await (const page of pages(dist)) {
  const html = await readFile(page, 'utf8');
  const name = page.slice(dist.length);
  for (const [tag] of html.matchAll(/<script\b[^>]*>/g)) {
    if (!/\bsrc=/.test(tag)) problems.push(`${name}: inline script ${tag}`);
  }
  if (/<style\b/.test(html)) problems.push(`${name}: inline <style>`);
  for (const [tag] of html.matchAll(/<[a-z][^>]*\sstyle=[^>]*>/g)) {
    problems.push(`${name}: style attribute in ${tag.slice(0, 80)}`);
  }
  for (const [tag] of html.matchAll(/<[a-z][^>]*\son[a-z]+=[^>]*>/g)) {
    problems.push(`${name}: inline event handler in ${tag.slice(0, 80)}`);
  }
}

if (problems.length > 0) {
  console.error('The build is not compatible with the Content-Security-Policy:');
  for (const problem of problems) console.error(`  ${problem}`);
  process.exit(1);
}
console.log('dist: no inline scripts, styles or handlers');
