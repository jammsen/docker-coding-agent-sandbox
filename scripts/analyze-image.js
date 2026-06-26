#!/usr/bin/env node
'use strict';
/**
 * analyze-image — direct vLLM vision analysis for opencode / OMP.
 *
 * Reads an image file, sends it to vLLM as a base64 image_url vision request,
 * and prints the model's description to stdout.
 *
 * Usage: analyze-image <path-to-image>
 */

const fs   = require('fs');
const http = require('http');

const imagePath = process.argv[2];
if (!imagePath) {
  process.stderr.write('Usage: analyze-image <path-to-image> [prompt]\n');
  process.exit(1);
}
const userPrompt = process.argv[3] || 'Describe this image in detail. What do you see?';

const VLLM_URL = process.env.VLLM_URL;
if (!VLLM_URL) {
  process.stderr.write('Error: VLLM_URL is not set. Set it in compose.yml, e.g. VLLM_URL=http://host:8000/v1\n');
  process.exit(1);
}
const MODEL              = process.env.VLLM_MODEL            || 'qwen3.6-35b';
const REQUEST_TIMEOUT_MS = parseInt(process.env.VLLM_REQUEST_TIMEOUT_MS || '300000', 10); // 5 min

function mimeFromMagic(buf) {
  if (buf[0] === 0x89 && buf[1] === 0x50 && buf[2] === 0x4e && buf[3] === 0x47) return 'image/png';
  if (buf[0] === 0xff && buf[1] === 0xd8 && buf[2] === 0xff)                    return 'image/jpeg';
  if (buf[0] === 0x47 && buf[1] === 0x49 && buf[2] === 0x46)                    return 'image/gif';
  if (buf[0] === 0x52 && buf[1] === 0x49 && buf[2] === 0x46 && buf[3] === 0x46 &&
      buf.length > 11 &&
      buf[8] === 0x57 && buf[9] === 0x45 && buf[10] === 0x42 && buf[11] === 0x50) return 'image/webp';
  return null;
}

let imageData;
try {
  imageData = fs.readFileSync(imagePath);
} catch (e) {
  process.stderr.write('Error reading file: ' + e.message + '\n');
  process.exit(1);
}

const mime = mimeFromMagic(imageData);
if (!mime) {
  process.stderr.write('Error: ' + imagePath + ' is not a valid PNG, JPEG, GIF, or WEBP image.\n');
  process.exit(1);
}
const dataUrl = 'data:' + mime + ';base64,' + imageData.toString('base64');

const payload = JSON.stringify({
  model: MODEL,
  messages: [{
    role:    'user',
    content: [
      { type: 'image_url', image_url: { url: dataUrl } },
      { type: 'text',      text: userPrompt },
    ],
  }],
  max_tokens: 2048,
});

const base = VLLM_URL.endsWith('/') ? VLLM_URL : VLLM_URL + '/';
const upstreamUrl = new URL('chat/completions', base);
const options = {
  hostname: upstreamUrl.hostname,
  port:     parseInt(upstreamUrl.port || '80', 10),
  path:     upstreamUrl.pathname,
  method:   'POST',
  headers:  {
    'Content-Type':   'application/json',
    'Authorization':  'Bearer dummy',
    'Content-Length': Buffer.byteLength(payload),
  },
};

const req = http.request(options, (res) => {
  let data = '';
  res.on('data', chunk => { data += chunk; });
  res.on('end', () => {
    try {
      const result = JSON.parse(data);
      if (result.error) {
        process.stderr.write('API error: ' + JSON.stringify(result.error) + '\n');
        process.exit(1);
      }
      const content = result.choices?.[0]?.message?.content;
      if (content) {
        process.stdout.write(content + '\n');
      } else {
        process.stderr.write('Unexpected response: ' + data + '\n');
        process.exit(1);
      }
    } catch (e) {
      process.stderr.write('Parse error: ' + e.message + '\n' + data + '\n');
      process.exit(1);
    }
  });
});

req.setTimeout(REQUEST_TIMEOUT_MS, () => {
  req.destroy(new Error(`request timeout after ${REQUEST_TIMEOUT_MS}ms`));
});

req.on('error', (e) => {
  process.stderr.write('Request error: ' + e.message + '\n');
  process.exit(1);
});

req.write(payload);
req.end();
