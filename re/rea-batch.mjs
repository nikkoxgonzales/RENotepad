// rea-batch.mjs — run many REA tool calls against ONE Ghidra import.
// Usage (target must sit on NTFS, e.g. under %TEMP%):
//   set GHIDRA_INSTALL_DIR=<your Ghidra install>
//   node re/rea-batch.mjs .tools/rea/node_modules/.bin/rea batch.json
// batch.json: [{"name":"open_binary","args":{"path":"C:\...\notepad.exe","provider_id":"ghidra"},"out":"out/open.json"},
//              {"name":"list_strings","args":{},"out":"out/strings.json"}, ...]
// Special entry {"name":"__callers","args":{"calls":[["xrefs",{"address":"EXTERNAL:0x106"}]]},"out":...}
// runs each call and batch-decompiles every containing procedure it can identify.
// With no batch file, writes the tool list (tools/list) to the third argument.
import { spawn } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';

const [, , reaBin, batchFile] = process.argv;
const batch = batchFile ? JSON.parse(readFileSync(batchFile, 'utf8')) : [];
const child = spawn(reaBin, ['--mcp'], { stdio: ['pipe', 'pipe', 'inherit'], shell: true, env: process.env });
let buf = '';
const pending = new Map();
let id = 0;
child.stdout.on('data', (d) => {
  buf += d.toString();
  let i;
  while ((i = buf.indexOf('\n')) >= 0) {
    const line = buf.slice(0, i).trim();
    buf = buf.slice(i + 1);
    if (!line) continue;
    let msg;
    try { msg = JSON.parse(line); } catch { continue; }
    if (msg.id != null && pending.has(msg.id)) { pending.get(msg.id)(msg); pending.delete(msg.id); }
  }
});
const rpc = (method, params) => new Promise((res) => {
  const myId = ++id;
  pending.set(myId, res);
  child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id: myId, method, params }) + '\n');
});

await rpc('initialize', { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'np', version: '1' } });
child.stdin.write(JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' }) + '\n');
if (!batch.length) {
  const tools = await rpc('tools/list', {});
  writeFileSync(process.argv[4] || 'tools.json', JSON.stringify(tools, null, 1));
} else {
  // Collect addresses of internal procedures containing references, from any result JSON.
  const callers = (obj, acc = new Set()) => {
    if (obj && typeof obj === 'object') {
      if (obj.containing_procedure?.procedure?.address) {
        const p = obj.containing_procedure.procedure;
        if (!p.classification?.external) acc.add(p.address);
      }
      if (typeof obj.name === 'string' && /^FUN_/.test(obj.name) && obj.address) acc.add(obj.address);
      for (const v of Object.values(obj)) callers(v, acc);
    }
    return acc;
  };
  for (const c of batch) {
    if (c.name === '__callers') {
      // find_xrefs_to_name for each import, then decompile every caller once
      const addrs = new Set();
      const map = {}; const raws = {};
      for (const [tool, targs] of c.args.calls) {
        const n = JSON.stringify(targs);
        const r = await rpc('tools/call', { name: tool, arguments: targs });
        const a = [...callers(r.result?.structuredContent)];
        map[n] = a; raws[n] = r.result?.structuredContent;
        a.forEach((x) => addrs.add(x));
      }
      const extra = c.args.extra || [];
      extra.forEach((x) => addrs.add(x));
      const d = await rpc('tools/call', { name: 'batch_decompile', arguments: { addresses: [...addrs] } });
      writeFileSync(c.out, JSON.stringify({ map, decompile: d }, null, 1)); writeFileSync(c.out + '.raw.json', JSON.stringify(raws, null, 1));
      console.error(`__callers -> ${c.out} (${addrs.size} procs)`);
      continue;
    }
    const t0 = Date.now();
    const r = await rpc('tools/call', { name: c.name, arguments: c.args });
    writeFileSync(c.out, JSON.stringify(r, null, 1));
    console.error(`${c.name} -> ${c.out} (${Date.now() - t0} ms)`);
  }
}
child.kill();
process.exit(0);
