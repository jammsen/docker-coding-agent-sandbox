#!/usr/bin/env node
// claude-shim — request-rewriting reverse proxy between Claude Code and LiteLLM.
//
// Why this exists:
//   Claude Code's Read tool delivers images as Anthropic `tool_result` blocks. When LiteLLM
//   translates an Anthropic /v1/messages request to the OpenAI chat/completions format that
//   vLLM speaks, it DROPS images nested inside tool_result blocks (OpenAI tool-role messages
//   cannot carry images). The model then receives an empty tool result and hallucinates.
//
//   This shim rewrites each request before LiteLLM sees it: any image inside a tool_result is
//   lifted out into a fresh user message (a placement vLLM handles correctly), with a text
//   placeholder left in the tool_result so the tool-call/result pairing stays valid. Everything
//   else — including streaming SSE responses and all non-/v1/messages paths — is proxied verbatim.
//
// Pure Node stdlib (no deps), matching upload-server.js. Listens on 127.0.0.1:SHIM_PORT and
// forwards to LITELLM_UPSTREAM (default http://agentic-litellm:4000).

const http = require('http');
const { URL } = require('url');

const SHIM_PORT = parseInt(process.env.CLAUDE_SHIM_PORT || '4001', 10);
const UPSTREAM = new URL(process.env.LITELLM_UPSTREAM || 'http://agentic-litellm:4000');

// --- the rewrite ---------------------------------------------------------
// Walk messages; for every user message, pull image blocks out of tool_result blocks and append
// them in a new user message right after. Returns true if anything changed.
function hoistToolResultImages(body) {
  if (!body || !Array.isArray(body.messages)) return false;
  let changed = false;
  const out = [];
  for (const msg of body.messages) {
    out.push(msg);
    if (!msg || msg.role !== 'user' || !Array.isArray(msg.content)) continue;
    const hoisted = [];
    for (const block of msg.content) {
      if (block && block.type === 'tool_result' && Array.isArray(block.content)) {
        const kept = [];
        for (const sub of block.content) {
          if (sub && sub.type === 'image') {
            hoisted.push(sub);
            kept.push({ type: 'text', text: '[image returned by tool — provided in the next message]' });
            changed = true;
          } else {
            kept.push(sub);
          }
        }
        block.content = kept;
      }
    }
    if (hoisted.length) {
      out.push({
        role: 'user',
        content: [{ type: 'text', text: 'Image(s) returned by the tool call above:' }, ...hoisted],
      });
    }
  }
  if (changed) body.messages = out;
  return changed;
}

// Apply only to JSON requests that carry a `messages` array (/v1/messages and its count_tokens
// variant). Returns a Buffer to forward, or null to forward the original bytes unchanged.
function maybeRewrite(pathname, raw) {
  if (!pathname.startsWith('/v1/messages')) return null;
  let body;
  try { body = JSON.parse(raw.toString('utf8')); } catch { return null; }
  if (!hoistToolResultImages(body)) return null;
  return Buffer.from(JSON.stringify(body), 'utf8');
}

const server = http.createServer((req, res) => {
  const chunks = [];
  req.on('data', (c) => chunks.push(c));
  req.on('end', () => {
    const raw = Buffer.concat(chunks);
    const pathname = req.url.split('?')[0];
    const rewritten = (req.method === 'POST') ? maybeRewrite(pathname, raw) : null;
    const outBody = rewritten || raw;

    const headers = { ...req.headers, host: UPSTREAM.host };
    if (outBody.length || req.method === 'POST') headers['content-length'] = Buffer.byteLength(outBody);
    delete headers['transfer-encoding'];

    const upstreamReq = http.request(
      {
        protocol: UPSTREAM.protocol,
        hostname: UPSTREAM.hostname,
        port: UPSTREAM.port || 80,
        method: req.method,
        path: req.url,
        headers,
      },
      (upstreamRes) => {
        res.writeHead(upstreamRes.statusCode || 502, upstreamRes.headers);
        upstreamRes.pipe(res); // streams SSE transparently
      }
    );
    upstreamReq.on('error', (err) => {
      res.writeHead(502, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ error: { type: 'shim_upstream_error', message: String(err) } }));
    });
    if (outBody.length) upstreamReq.write(outBody);
    upstreamReq.end();
  });
});

server.listen(SHIM_PORT, '127.0.0.1', () => {
  console.log(`> claude-shim listening on 127.0.0.1:${SHIM_PORT} → ${UPSTREAM.origin} (hoisting tool_result images)`);
});
