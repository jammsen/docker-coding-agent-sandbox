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
const path = require('path');

const imagePath = process.argv[2];
if (!imagePath) {
  process.stderr.write('Usage: analyze-image <path-to-image> [prompt]\n');
  process.exit(1);
}
const userPrompt = process.argv[3] || 'Describe this image in detail. What do you see?';

const VLLM_BASE = process.env.VLLM_URL   || 'http://10.0.0.13:8000';
const MODEL     = process.env.VLLM_MODEL || 'qwen3.6-35b';

const MIME_MAP = {
  png:  'image/png',
  jpg:  'image/jpeg',
  jpeg: 'image/jpeg',
  gif:  'image/gif',
  webp: 'image/webp',
};

let imageData;
try {
  imageData = fs.readFileSync(imagePath);
} catch (e) {
  process.stderr.write('Error reading file: ' + e.message + '\n');
  process.exit(1);
}

const ext  = path.extname(imagePath).slice(1).toLowerCase();
const mime = MIME_MAP[ext] || 'image/png';
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

const upstreamUrl = new URL(VLLM_BASE + '/v1/chat/completions');
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

req.on('error', (e) => {
  process.stderr.write('Request error: ' + e.message + '\n');
  process.exit(1);
});

req.write(payload);
req.end();
