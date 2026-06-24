#!/usr/bin/env node
'use strict';
/**
 * upload-server.js — image upload companion for the agentic harness sandbox.
 *
 * Pure Node.js stdlib — zero npm packages.
 * Reuses the same self-signed TLS cert as wetty so the browser grants
 * a secure context (required for the Copy button to work).
 *
 * Listens on port 1112.
 * Saves uploads to UPLOAD_DIR (/home/agent/workspace/uploads by default).
 */

const http  = require('http');
const https = require('https');
const fs    = require('fs');
const path  = require('path');

const PORT       = parseInt(process.env.UPLOAD_PORT || '1112', 10);
const UPLOAD_DIR = process.env.UPLOAD_DIR || '/home/agent/workspace/uploads';
const MAX_BYTES  = 50 * 1024 * 1024; // 50 MB
const SSL_KEY    = process.env.SSL_KEY  || '/etc/wetty/key.pem';
const SSL_CERT   = process.env.SSL_CERT || '/etc/wetty/cert.pem';

fs.mkdirSync(UPLOAD_DIR, { recursive: true });

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Detect image type by magic bytes. Returns 'png'|'jpg'|'gif'|'webp' or null. */
function imageExt(buf) {
  if (buf.length < 12) return null;
  if (buf[0] === 0x89 && buf[1] === 0x50 && buf[2] === 0x4E && buf[3] === 0x47) return 'png';
  if (buf[0] === 0xFF && buf[1] === 0xD8 && buf[2] === 0xFF)                    return 'jpg';
  if (buf[0] === 0x47 && buf[1] === 0x49 && buf[2] === 0x46)                    return 'gif';
  if (buf[0] === 0x52 && buf[1] === 0x49 && buf[2] === 0x46 && buf[3] === 0x46
   && buf[8] === 0x57 && buf[9] === 0x45 && buf[10] === 0x42 && buf[11] === 0x50) return 'webp';
  return null;
}

/** YYYY-MM-DD-HH-MM-SS (local time) */
function timestamp() {
  const d = new Date();
  const z = n => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${z(d.getMonth()+1)}-${z(d.getDate())}`
       + `-${z(d.getHours())}-${z(d.getMinutes())}-${z(d.getSeconds())}`;
}

/** Send a JSON response. */
function json(res, status, obj) {
  const body = Buffer.from(JSON.stringify(obj), 'utf8');
  res.writeHead(status, {
    'Content-Type': 'application/json',
    'Content-Length': body.length,
    'X-Content-Type-Options': 'nosniff',
  });
  res.end(body);
}

// ---------------------------------------------------------------------------
// HTML page (embedded — no separate file needed)
// ---------------------------------------------------------------------------

const HTML = `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Screenshot Upload — Sandbox</title>
  <style>
    *, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }

    body {
      font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
      background: #0f1117;
      color: #e2e8f0;
      min-height: 100vh;
      display: flex;
      flex-direction: column;
      align-items: center;
      justify-content: center;
      gap: 24px;
      padding: 24px;
    }

    .card {
      background: #1a1d2e;
      border: 1px solid #2d3148;
      border-radius: 12px;
      padding: 32px;
      width: 100%;
      max-width: 640px;
      box-shadow: 0 8px 32px rgba(0,0,0,0.4);
    }

    h1 {
      font-size: 1.2rem;
      font-weight: 600;
      color: #a5b4fc;
      margin-bottom: 6px;
    }

    .subtitle {
      font-size: 0.85rem;
      color: #475569;
      margin-bottom: 24px;
    }

    /* Drop zone */
    .drop-zone {
      border: 2px dashed #3d4263;
      border-radius: 8px;
      padding: 44px 24px;
      text-align: center;
      cursor: pointer;
      transition: border-color 0.15s, background 0.15s;
      user-select: none;
    }

    .drop-zone:hover {
      border-color: #6366f1;
      background: rgba(99,102,241,0.06);
    }

    .drop-zone.drag-over {
      border-color: #818cf8;
      border-style: solid;
      background: rgba(99,102,241,0.12);
    }

    .drop-icon { font-size: 2.4rem; margin-bottom: 10px; line-height: 1; }

    .drop-label { font-size: 0.95rem; color: #94a3b8; }
    .drop-label strong { color: #a5b4fc; }
    .drop-hint { font-size: 0.78rem; color: #475569; margin-top: 6px; }

    /* Paste hint bar */
    .paste-hint {
      margin-top: 10px;
      padding: 8px 14px;
      background: rgba(99,102,241,0.08);
      border: 1px solid #2d3148;
      border-radius: 6px;
      font-size: 0.82rem;
      color: #818cf8;
      text-align: center;
    }

    .paste-hint kbd {
      background: #2d3148;
      border: 1px solid #3d4263;
      border-radius: 4px;
      padding: 1px 5px;
      font-family: monospace;
      font-size: 0.8rem;
    }

    /* Preview */
    #preview {
      display: none;
      margin-top: 18px;
      border: 1px solid #2d3148;
      border-radius: 8px;
      overflow: hidden;
      background: #0f1117;
    }

    #previewImg {
      display: block;
      max-width: 100%;
      max-height: 300px;
      margin: 0 auto;
    }

    #previewMeta {
      padding: 7px 12px;
      font-size: 0.76rem;
      color: #64748b;
      border-top: 1px solid #2d3148;
    }

    /* Actions */
    .actions {
      display: flex;
      gap: 10px;
      margin-top: 16px;
    }

    button {
      font-size: 0.88rem;
      font-weight: 500;
      border: none;
      border-radius: 6px;
      cursor: pointer;
      padding: 10px 20px;
      transition: opacity 0.15s, background 0.15s;
    }

    button:disabled { opacity: 0.4; cursor: not-allowed; }

    #btnUpload {
      background: #6366f1;
      color: #fff;
      flex: 1;
    }
    #btnUpload:not(:disabled):hover { background: #4f46e5; }

    #btnClear {
      background: #2d3148;
      color: #94a3b8;
    }
    #btnClear:hover { background: #3d4263; }

    /* Status */
    #uploading {
      display: none;
      margin-top: 12px;
      font-size: 0.85rem;
      color: #64748b;
      text-align: center;
    }

    #errorMsg {
      display: none;
      margin-top: 12px;
      padding: 10px 14px;
      background: rgba(239,68,68,0.10);
      border: 1px solid rgba(239,68,68,0.30);
      border-radius: 6px;
      font-size: 0.85rem;
      color: #fca5a5;
    }

    /* Result */
    #result {
      display: none;
      margin-top: 18px;
      padding: 16px;
      background: rgba(34,197,94,0.07);
      border: 1px solid rgba(34,197,94,0.22);
      border-radius: 8px;
    }

    #result p {
      font-size: 0.85rem;
      color: #86efac;
      margin-bottom: 10px;
    }

    .path-box {
      display: flex;
      align-items: center;
      gap: 8px;
      background: #0f1117;
      border: 1px solid #2d3148;
      border-radius: 6px;
      padding: 8px 12px;
    }

    #resultPath {
      font-family: 'Consolas', 'Fira Mono', monospace;
      font-size: 0.84rem;
      color: #a5f3fc;
      flex: 1;
      word-break: break-all;
    }

    #btnCopy {
      background: #2d3148;
      color: #94a3b8;
      padding: 6px 12px;
      font-size: 0.78rem;
      flex-shrink: 0;
    }
    #btnCopy:hover { background: #3d4263; }
    #btnCopy.ok    { color: #86efac; }

    .result-row { margin-top: 10px; }
    .result-row:first-child { margin-top: 0; }

    .result-label {
      font-size: 0.78rem;
      color: #64748b;
      margin-bottom: 5px;
      text-transform: uppercase;
      letter-spacing: 0.05em;
    }

    #btnCopyPrompt {
      background: #2d3148;
      color: #94a3b8;
      padding: 6px 12px;
      font-size: 0.78rem;
      flex-shrink: 0;
    }
    #btnCopyPrompt:hover { background: #3d4263; }
    #btnCopyPrompt.ok    { color: #86efac; }

    /* Gallery card */
    .gallery-card {
      background: rgba(99, 102, 241, 0.05);
      border-color: rgba(99, 102, 241, 0.25);
    }
    .gallery-header {
      display: flex;
      align-items: center;
      justify-content: space-between;
      margin-bottom: 14px;
    }
    .gallery-header h2 { font-size: 1rem; font-weight: 600; color: #818cf8; }
    #btnRefresh {
      background: transparent;
      border: 1px solid #2d3148;
      color: #64748b;
      padding: 5px 12px;
      font-size: 0.82rem;
      border-radius: 6px;
    }
    #btnRefresh:hover { background: #2d3148; color: #94a3b8; }
    .gallery-empty {
      text-align: center;
      padding: 24px 0;
      font-size: 0.85rem;
      color: #475569;
    }
    .gallery-grid {
      display: flex;
      flex-wrap: wrap;
      justify-content: center;
      gap: 8px;
    }
    .thumb {
      width: 96px;
      aspect-ratio: 1;
      border-radius: 6px;
      overflow: hidden;
      cursor: pointer;
      border: 2px solid transparent;
      background: #0f1117;
      transition: border-color 0.15s, transform 0.1s;
    }
    .thumb:hover { border-color: #6366f1; transform: scale(1.04); }
    .thumb img { width: 100%; height: 100%; object-fit: cover; display: block; }

    /* Modal */
    .modal {
      display: none;
      position: fixed;
      inset: 0;
      z-index: 200;
      align-items: center;
      justify-content: center;
      padding: 24px;
    }
    .modal.open { display: flex; }
    .modal-backdrop {
      position: absolute;
      inset: 0;
      background: rgba(0,0,0,0.78);
      backdrop-filter: blur(4px);
      cursor: pointer;
    }
    .modal-box {
      position: relative;
      background: #1a1d2e;
      border: 1px solid #2d3148;
      border-radius: 12px;
      padding: 28px 24px 24px;
      width: 100%;
      max-width: 700px;
      max-height: 90vh;
      overflow-y: auto;
      box-shadow: 0 20px 60px rgba(0,0,0,0.7);
      z-index: 1;
    }
    #btnModalClose {
      position: absolute;
      top: 12px;
      right: 12px;
      background: #2d3148;
      color: #94a3b8;
      border: none;
      border-radius: 6px;
      width: 30px;
      height: 30px;
      font-size: 1rem;
      cursor: pointer;
      padding: 0;
      line-height: 30px;
      text-align: center;
    }
    #btnModalClose:hover { background: #3d4263; }
    #modalImg {
      display: block;
      max-width: 100%;
      max-height: 55vh;
      margin: 0 auto 8px;
      border-radius: 6px;
      object-fit: contain;
      background: #0f1117;
    }
    .modal-name {
      font-size: 0.75rem;
      color: #475569;
      text-align: center;
      margin-bottom: 16px;
      font-family: 'Consolas', 'Fira Mono', monospace;
      word-break: break-all;
    }
    #btnModalCopyPath { background: #2d3148; color: #94a3b8; padding: 6px 12px; font-size: 0.78rem; flex-shrink: 0; }
    #btnModalCopyPath:hover { background: #3d4263; }
    #btnModalCopyPath.ok { color: #86efac; }
    #btnModalCopyPrompt { background: #2d3148; color: #94a3b8; padding: 6px 12px; font-size: 0.78rem; flex-shrink: 0; }
    #btnModalCopyPrompt:hover { background: #3d4263; }
    #btnModalCopyPrompt.ok { color: #86efac; }
    .modal-delete-row {
      margin-top: 18px;
      display: flex;
      justify-content: center;
    }
    #btnModalDelete {
      background: rgba(239,68,68,0.12);
      color: #fca5a5;
      border: 1px solid rgba(239,68,68,0.35);
      padding: 8px 28px;
      font-size: 0.88rem;
      border-radius: 6px;
    }
    #btnModalDelete:hover { background: rgba(239,68,68,0.22); border-color: rgba(239,68,68,0.6); color: #fecaca; }
    #btnModalDelete.confirming { background: rgba(239,68,68,0.80); color: #fff; border-color: transparent; }
  </style>
</head>
<body>
<div class="card">
  <h1>Screenshot Upload</h1>
  <p class="subtitle">Upload an image, then paste the path into your agent terminal.</p>

  <div class="drop-zone" id="dropZone">
    <div class="drop-icon">🖼</div>
    <div class="drop-label"><strong>Click to browse</strong> or drag &amp; drop here</div>
    <div class="drop-hint">PNG &middot; JPEG &middot; GIF &middot; WEBP &mdash; max 50 MB</div>
  </div>
  <input type="file" id="fileInput" accept="image/*" style="display:none">

  <div class="paste-hint">
    Or press <kbd>Ctrl</kbd>+<kbd>V</kbd> anywhere on this page to paste a screenshot from clipboard
  </div>

  <div id="preview">
    <img id="previewImg" src="" alt="preview">
    <div id="previewMeta"></div>
  </div>

  <div class="actions">
    <button id="btnUpload" disabled>Upload</button>
    <button id="btnClear">Clear</button>
  </div>

  <div id="uploading">Uploading\u2026</div>
  <div id="errorMsg"></div>

  <div id="result">
    <div class="result-row">
      <div class="result-label">Path</div>
      <div class="path-box">
        <span id="resultPath"></span>
        <button id="btnCopy">Copy</button>
      </div>
    </div>
    <div class="result-row">
      <div class="result-label">Prompt</div>
      <div class="path-box">
        <span id="resultPrompt"></span>
        <button id="btnCopyPrompt">Copy</button>
      </div>
    </div>
  </div>
</div>

<div class="card gallery-card">
  <div class="gallery-header">
    <h2>Recent Uploads</h2>
    <button id="btnRefresh">↻ Refresh</button>
  </div>
  <div id="galleryEmpty" class="gallery-empty">No images uploaded yet.</div>
  <div id="galleryGrid" class="gallery-grid" style="display:none"></div>
</div>

<div class="modal" id="modal">
  <div class="modal-backdrop" id="modalBackdrop"></div>
  <div class="modal-box">
    <button id="btnModalClose">✕</button>
    <img id="modalImg" src="" alt="">
    <div class="modal-name" id="modalName"></div>
    <div class="result-row">
      <div class="result-label">Path</div>
      <div class="path-box">
        <span id="modalPath" style="font-family:'Consolas','Fira Mono',monospace;font-size:0.84rem;color:#a5f3fc;flex:1;word-break:break-all;"></span>
        <button id="btnModalCopyPath">Copy</button>
      </div>
    </div>
    <div class="result-row">
      <div class="result-label">Prompt</div>
      <div class="path-box">
        <span id="modalPrompt" style="font-size:0.84rem;flex:1;word-break:break-all;"></span>
        <button id="btnModalCopyPrompt">Copy</button>
      </div>
    </div>
    <div class="modal-delete-row">
      <button id="btnModalDelete">🗑 Delete image</button>
    </div>
  </div>
</div>

<script>
  'use strict';

  let selectedFile = null;

  const dropZone    = document.getElementById('dropZone');
  const fileInput   = document.getElementById('fileInput');
  const preview     = document.getElementById('preview');
  const previewImg  = document.getElementById('previewImg');
  const previewMeta = document.getElementById('previewMeta');
  const btnUpload   = document.getElementById('btnUpload');
  const btnClear    = document.getElementById('btnClear');
  const uploading   = document.getElementById('uploading');
  const errorMsg    = document.getElementById('errorMsg');
  const result        = document.getElementById('result');
  const resultPath    = document.getElementById('resultPath');
  const resultPrompt  = document.getElementById('resultPrompt');
  const btnCopy       = document.getElementById('btnCopy');
  const btnCopyPrompt = document.getElementById('btnCopyPrompt');

  function fmtBytes(n) {
    if (n < 1024) return n + ' B';
    if (n < 1048576) return (n / 1024).toFixed(1) + ' KB';
    return (n / 1048576).toFixed(1) + ' MB';
  }

  function showError(msg) {
    errorMsg.textContent = msg;
    errorMsg.style.display = 'block';
  }

  function clearError() { errorMsg.style.display = 'none'; }

  function handleFile(file) {
    if (!file) return;
    if (!file.type.startsWith('image/')) { showError('Not an image file.'); return; }
    if (file.size > 50 * 1024 * 1024)   { showError('File exceeds 50 MB limit.'); return; }

    selectedFile = file;
    clearError();
    result.style.display = 'none';

    const reader = new FileReader();
    reader.onload = e => {
      previewImg.src = e.target.result;
      previewMeta.textContent = file.name + '  \u2014  ' + fmtBytes(file.size);
      preview.style.display = 'block';
    };
    reader.readAsDataURL(file);
    btnUpload.disabled = false;
  }

  function clear() {
    selectedFile = null;
    fileInput.value = '';
    previewImg.src = '';
    preview.style.display = 'none';
    previewMeta.textContent = '';
    btnUpload.disabled = true;
    result.style.display = 'none';
    clearError();
  }

  async function doUpload() {
    if (!selectedFile) return;
    btnUpload.disabled = true;
    uploading.style.display = 'block';
    clearError();
    result.style.display = 'none';

    try {
      const res = await fetch('/upload', {
        method: 'POST',
        headers: { 'Content-Type': selectedFile.type },
        body: selectedFile,
      });
      const data = await res.json();
      if (!res.ok || data.error) {
        showError(data.error || 'Upload failed (status ' + res.status + ')');
        btnUpload.disabled = false;
      } else {
        resultPath.textContent = data.path;
        resultPrompt.textContent = 'Analyze the image at ' + data.path + ' and describe what you see.';
        result.style.display = 'block';
        loadGallery();
      }
    } catch (err) {
      showError('Network error: ' + err.message);
      btnUpload.disabled = false;
    } finally {
      uploading.style.display = 'none';
    }
  }

  // --- Events ---

  // Click drop zone → open file picker
  dropZone.addEventListener('click', () => fileInput.click());

  fileInput.addEventListener('change', () => {
    if (fileInput.files[0]) handleFile(fileInput.files[0]);
  });

  // Drag and drop
  dropZone.addEventListener('dragover', e => {
    e.preventDefault();
    dropZone.classList.add('drag-over');
  });
  ['dragleave', 'dragend'].forEach(ev =>
    dropZone.addEventListener(ev, () => dropZone.classList.remove('drag-over'))
  );
  dropZone.addEventListener('drop', e => {
    e.preventDefault();
    dropZone.classList.remove('drag-over');
    if (e.dataTransfer.files[0]) handleFile(e.dataTransfer.files[0]);
  });

  // Ctrl+V — paste from clipboard (works with WIN+SHIFT+S screenshots)
  document.addEventListener('paste', e => {
    const items = Array.from(e.clipboardData ? e.clipboardData.items : []);
    const img = items.find(i => i.type.startsWith('image/'));
    if (img) handleFile(img.getAsFile());
  });

  btnUpload.addEventListener('click', doUpload);
  btnClear.addEventListener('click', clear);

  // Copy helpers
  function copyText(text, btn) {
    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(text).then(() => flash(btn)).catch(() => fallbackCopy(text, btn));
    } else {
      fallbackCopy(text, btn);
    }
  }

  function fallbackCopy(text, btn) {
    const ta = document.createElement('textarea');
    ta.value = text;
    ta.style.position = 'fixed';
    ta.style.opacity = '0';
    document.body.appendChild(ta);
    ta.focus();
    ta.select();
    document.execCommand('copy');
    document.body.removeChild(ta);
    flash(btn);
  }

  btnCopy.addEventListener('click', () => copyText(resultPath.textContent, btnCopy));
  btnCopyPrompt.addEventListener('click', () => copyText(resultPrompt.textContent, btnCopyPrompt));

  function flash(btn) {
    btn.textContent = 'Copied!';
    btn.classList.add('ok');
    setTimeout(() => { btn.textContent = 'Copy'; btn.classList.remove('ok'); }, 2000);
  }

  // --- Gallery ---
  var galleryGrid  = document.getElementById('galleryGrid');
  var galleryEmpty = document.getElementById('galleryEmpty');

  async function loadGallery() {
    try {
      var res = await fetch('/images');
      var data = await res.json();
      renderGallery(data.images || []);
    } catch { /* ignore */ }
  }

  function renderGallery(images) {
    galleryGrid.innerHTML = '';
    if (!images.length) {
      galleryEmpty.style.display = 'block';
      galleryGrid.style.display = 'none';
      return;
    }
    galleryEmpty.style.display = 'none';
    galleryGrid.style.display = 'flex';
    images.forEach(function(item) {
      var div = document.createElement('div');
      div.className = 'thumb';
      var img = document.createElement('img');
      img.src = '/image/' + encodeURIComponent(item.filename);
      img.alt = item.filename;
      img.loading = 'lazy';
      div.appendChild(img);
      div.addEventListener('click', (function(p, f, s) {
        return function() { openModal(p, f, s); };
      })(item.path, item.filename, item.size));
      galleryGrid.appendChild(div);
    });
  }

  document.getElementById('btnRefresh').addEventListener('click', loadGallery);

  // --- Modal ---
  var modalEl            = document.getElementById('modal');
  var modalImgEl         = document.getElementById('modalImg');
  var modalNameEl        = document.getElementById('modalName');
  var modalPathEl        = document.getElementById('modalPath');
  var modalPromptEl      = document.getElementById('modalPrompt');
  var btnModalCopyPath   = document.getElementById('btnModalCopyPath');
  var btnModalCopyPrompt = document.getElementById('btnModalCopyPrompt');
  var btnModalDelete     = document.getElementById('btnModalDelete');
  var _modalCurrentFile  = null; // filename currently shown in modal

  function fmtSize(bytes) {
    if (bytes < 1024) return bytes + ' B';
    if (bytes < 1048576) return (bytes / 1024).toFixed(1) + ' KB';
    return (bytes / 1048576).toFixed(1) + ' MB';
  }

  function openModal(imgPath, filename, size) {
    modalImgEl.src = '/image/' + encodeURIComponent(filename);
    modalNameEl.textContent = filename;
    // Dimensions filled in once image is loaded
    modalImgEl.onload = function() {
      var dims = modalImgEl.naturalWidth + ' × ' + modalImgEl.naturalHeight + ' px';
      var sizeStr = size != null ? '  ·  ' + fmtSize(size) : '';
      modalNameEl.textContent = filename + '  ·  ' + dims + sizeStr;
    };
    modalPathEl.textContent = imgPath;
    modalPromptEl.textContent = 'Analyze the image at ' + imgPath + ' and describe what you see.';
    _modalCurrentFile = filename;
    btnModalDelete.textContent = '\uD83D\uDDD1 Delete image';
    btnModalDelete.classList.remove('confirming');
    modalEl.classList.add('open');
    document.body.style.overflow = 'hidden';
  }

  function closeModal() {
    modalEl.classList.remove('open');
    document.body.style.overflow = '';
    setTimeout(function() { modalImgEl.src = ''; }, 200);
  }

  document.getElementById('btnModalClose').addEventListener('click', closeModal);
  document.getElementById('modalBackdrop').addEventListener('click', closeModal);
  document.addEventListener('keydown', function(e) { if (e.key === 'Escape') closeModal(); });

  btnModalCopyPath.addEventListener('click', function() { copyText(modalPathEl.textContent, btnModalCopyPath); });
  btnModalCopyPrompt.addEventListener('click', function() { copyText(modalPromptEl.textContent, btnModalCopyPrompt); });

  // Delete — two-step: first click asks to confirm, second click deletes
  btnModalDelete.addEventListener('click', function() {
    if (!btnModalDelete.classList.contains('confirming')) {
      btnModalDelete.textContent = 'Confirm delete?';
      btnModalDelete.classList.add('confirming');
      // Auto-reset after 3 s if no second click
      setTimeout(function() {
        btnModalDelete.textContent = '\uD83D\uDDD1 Delete image';
        btnModalDelete.classList.remove('confirming');
      }, 3000);
      return;
    }
    var filename = _modalCurrentFile;
    if (!filename) return;
    fetch('/image/' + encodeURIComponent(filename), { method: 'DELETE' })
      .then(function(r) { return r.json(); })
      .then(function(data) {
        if (data.error) { alert('Delete failed: ' + data.error); return; }
        closeModal();
        loadGallery();
      })
      .catch(function() { alert('Delete request failed.'); });
  });

  loadGallery();
</script>
</body>
</html>`;

// ---------------------------------------------------------------------------
// HTTP request handler
// ---------------------------------------------------------------------------

function handleRequest(req, res) {
  // Serve the upload page
  if (req.method === 'GET' && (req.url === '/' || req.url === '/index.html')) {
    const body = Buffer.from(HTML, 'utf8');
    res.writeHead(200, {
      'Content-Type': 'text/html; charset=utf-8',
      'Content-Length': body.length,
      'X-Content-Type-Options': 'nosniff',
      'X-Frame-Options': 'DENY',
      'Referrer-Policy': 'no-referrer',
    });
    return res.end(body);
  }

  // List uploaded images — newest first
  if (req.method === 'GET' && req.url === '/images') {
    const exts = new Set(['png', 'jpg', 'gif', 'webp']);
    try {
      const images = fs.readdirSync(UPLOAD_DIR)
        .filter(f => exts.has(path.extname(f).slice(1).toLowerCase()))
        .map(f => ({ filename: f, path: path.join(UPLOAD_DIR, f), mtime: fs.statSync(path.join(UPLOAD_DIR, f)).mtimeMs, size: fs.statSync(path.join(UPLOAD_DIR, f)).size }))
        .sort((a, b) => b.mtime - a.mtime)
        .map(({ filename, path: fp, size }) => ({ filename, path: fp, size }));
      return json(res, 200, { images });
    } catch (err) {
      return json(res, 500, { error: 'Could not list images' });
    }
  }

  // Serve an uploaded image by filename (thumbnails + modal)
  const imgServeMatch = req.url.match(/^\/image\/([^?#]+)$/);
  if (req.method === 'GET' && imgServeMatch) {
    const rawName = decodeURIComponent(imgServeMatch[1]);
    // Strict allow-list: no path separators or traversal — spaces and parens are fine
    if (!/^[\w ()-]+\.(png|jpg|gif|webp)$/i.test(rawName)) {
      res.writeHead(400); return res.end();
    }
    const filePath = path.join(UPLOAD_DIR, rawName);
    const mimeMap = { png: 'image/png', jpg: 'image/jpeg', gif: 'image/gif', webp: 'image/webp' };
    const mime = mimeMap[path.extname(rawName).slice(1).toLowerCase()];
    try {
      const data = fs.readFileSync(filePath);
      res.writeHead(200, {
        'Content-Type': mime,
        'Content-Length': data.length,
        'Cache-Control': 'max-age=3600, immutable',
        'X-Content-Type-Options': 'nosniff',
      });
      return res.end(data);
    } catch {
      res.writeHead(404); return res.end();
    }
  }

  // Delete an uploaded image
  if (req.method === 'DELETE' && imgServeMatch) {
    const rawName = decodeURIComponent(imgServeMatch[1]);
    if (!/^[\w ()-]+\.(png|jpg|gif|webp)$/i.test(rawName)) {
      res.writeHead(400); return res.end();
    }
    const filePath = path.join(UPLOAD_DIR, rawName);
    // Defense-in-depth: confirm resolved path is inside UPLOAD_DIR
    if (!path.resolve(filePath).startsWith(path.resolve(UPLOAD_DIR) + path.sep)) {
      res.writeHead(403); return res.end();
    }
    try {
      fs.unlinkSync(filePath);
      console.log('[upload-server] deleted:', filePath);
      return json(res, 200, { ok: true });
    } catch (err) {
      return json(res, 404, { error: 'File not found or already deleted' });
    }
  }

  // Accept an uploaded image
  if (req.method === 'POST' && req.url === '/upload') {
    const contentLength = parseInt(req.headers['content-length'] || '0', 10);
    if (isNaN(contentLength) || contentLength > MAX_BYTES) {
      return json(res, 413, { error: 'File too large (max 50 MB)' });
    }

    const chunks = [];
    let received = 0;

    req.on('data', chunk => {
      received += chunk.length;
      if (received > MAX_BYTES) { req.destroy(); return; }
      chunks.push(chunk);
    });

    req.on('end', () => {
      const buf = Buffer.concat(chunks);
      const ext = imageExt(buf);
      if (!ext) {
        return json(res, 400, { error: 'Not a valid image — PNG, JPEG, GIF or WEBP only' });
      }

      const dest = path.join(UPLOAD_DIR, `${timestamp()}.${ext}`);
      try {
        fs.writeFileSync(dest, buf);
      } catch (err) {
        console.error('[upload-server] write error:', err.message);
        return json(res, 500, { error: 'Failed to save file' });
      }

      console.log(`[upload-server] saved: ${dest}`);
      return json(res, 200, { path: dest });
    });

    req.on('error', () => json(res, 400, { error: 'Request stream error' }));
    return;
  }

  res.writeHead(404);
  res.end();
}

// ---------------------------------------------------------------------------
// Start server — HTTPS if certs exist (reuses wetty's self-signed cert),
// falls back to plain HTTP so the server always starts.
// ---------------------------------------------------------------------------

let server;
if (fs.existsSync(SSL_KEY) && fs.existsSync(SSL_CERT)) {
  const tlsOpts = {
    key:  fs.readFileSync(SSL_KEY),
    cert: fs.readFileSync(SSL_CERT),
  };
  server = https.createServer(tlsOpts, handleRequest);
  server.listen(PORT, '0.0.0.0', () =>
    console.log(`[upload-server] HTTPS on port ${PORT}, saving to ${UPLOAD_DIR}`)
  );
} else {
  server = http.createServer(handleRequest);
  server.listen(PORT, '0.0.0.0', () =>
    console.log(`[upload-server] HTTP on port ${PORT}, saving to ${UPLOAD_DIR}`)
  );
}
