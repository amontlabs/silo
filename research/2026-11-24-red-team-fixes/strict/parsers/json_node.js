// JSON.parse: what a Node host sees of a body. usage: node json_node.js <file>
const fs = require('fs'); let o = { ok: false, web_fetch: false, max_tokens: null };
try { const j = JSON.parse(fs.readFileSync(process.argv[2], 'utf8')); o = { ok: true, web_fetch: JSON.stringify(j.tools || []).includes('web_fetch'), max_tokens: String(j.max_tokens) }; } catch (e) {}
console.log(JSON.stringify(o));
