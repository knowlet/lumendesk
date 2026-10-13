import { spawnSync } from 'node:child_process';
import assert from 'node:assert/strict';
const compiler=process.env.TSC || 'tsc';
const positive=spawnSync(compiler,['-p','tests/typescript/tsconfig.json'],{encoding:'utf8'});
assert.equal(positive.status,0,`Positive TypeScript fixture failed: ${positive.error || positive.stdout || positive.stderr}`);
const negative=spawnSync(compiler,['--ignoreConfig','--strict','--noEmit','--lib','ES2022,DOM','--module','ESNext','--moduleResolution','bundler','tests/typescript/negative.ts'],{encoding:'utf8'});
assert.ok(negative.status === 1 || negative.status === 2,`Negative fixture should fail type checking: ${negative.error || negative.stdout || negative.stderr}`);
for(const line of [3,4,5,6]) assert.match(negative.stdout,new RegExp(`negative\\.ts\\(${line},`));
console.log('TypeScript: positive fixture passed; all four expected invalid uses rejected.');
