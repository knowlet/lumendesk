// Static consistency checks only; these do not render HTML or execute a browser.
import assert from 'node:assert/strict';
import { readFile, stat } from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

const assets = new URL('../assets/', import.meta.url);
const html = await readFile(new URL('index.html', assets), 'utf8');
const javascript = await readFile(new URL('app.js', assets), 'utf8');
const rust = await readFile(new URL('../crates/lumendesk/src/webview.rs', import.meta.url), 'utf8');
const scaffold = await readFile(new URL('../crates/lumendesk-cli/src/scaffold.rs', import.meta.url), 'utf8');

test('every sample frontend script ID names an actual element', () => {
  const ids = new Set(Array.from(html.matchAll(/\bid="([^"]+)"/g), match => match[1]));
  for (const [, id] of javascript.matchAll(/getElementById\('([^']+)'\)/g)) {
    assert.ok(ids.has(id), `missing sample element #${id}`);
  }
  assert.ok(html.includes('aria-live="polite"'));
  assert.match(html, /<button[^>]+id="submit"[^>]*\bdisabled\b/);
});

test('sample page resource links exist and are included in the Rust asset bundle', async () => {
  const paths = Array.from(html.matchAll(/\b(?:src|href)="\/([^"]+)"/g), match => match[1]);
  assert.ok(paths.length >= 3);
  for (const path of paths) {
    assert.ok((await stat(new URL(path, assets))).isFile(), `missing asset ${path}`);
    assert.ok(rust.includes(`"${path}"`), `Rust asset bundle does not include ${path}`);
  }
});

test('sample app JavaScript parses without a frontend build tool', () => {
  assert.doesNotThrow(() => new vm.Script(javascript, { filename: 'assets/app.js' }));
});

test('web scaffold contains parseable JavaScript and matches all literal ID selectors', () => {
  const match = scaffold.match(/const WEB_HTML: &str = r#"([\s\S]*?)"#;/);
  assert.ok(match, 'web scaffold raw-string template found');
  const template = match[1];
  const ids = new Set(Array.from(template.matchAll(/\bid="([^"]+)"/g), entry => entry[1]));
  const scripts = Array.from(template.matchAll(/<script>([\s\S]*?)<\/script>/g), entry => entry[1]);
  assert.equal(scripts.length, 1);
  for (const script of scripts) {
    assert.doesNotThrow(() => new vm.Script(script, { filename: 'scaffold/web/index.html' }));
    for (const [, id] of script.matchAll(/querySelector\('#([^']+)'\)/g)) {
      assert.ok(ids.has(id), `missing scaffold element #${id}`);
    }
  }
  assert.ok(template.includes('aria-live="polite"'));
});
