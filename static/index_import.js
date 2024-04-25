// @license magnet:?xt=urn:btih:0b31508aeb0634b347b8270c7bee4d411b5d4109&dn=agpl-3.0.txt AGPL-v3-or-Later
const importButton = document.getElementById('import-button');
const importModal = document.getElementById('import-modal');
const confirmImport = document.getElementById('confirm-import');
const payloadJson = document.getElementById('import-payload');
const importForm = importModal.querySelector('form');
const dropZone = document.getElementById('file-upload-drop-zone');
let lastDraggedTarget = null;

importModal?.querySelector('button[formmethod=dialog]').addEventListener('click', (e) => {
  e.preventDefault();
  importModal.close();
});

const throwHook = (alert) => { throw new Error(alert.querySelector('p').textContent) };

async function downloadZipFromUrl(url) {
  let blob = await callApi(`/download-zip?url=${encodeURIComponent(url)}`, undefined, throwHook, true);
  return await new zip.ZipReader(new zip.BlobReader(blob)).getEntries({});
}

async function blobToBase64(blob) {
  const url = await new Promise(resolve => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result);
    reader.readAsDataURL(blob);
  });
  return url.slice(url.indexOf(',') + 1);
}

async function compressJson(payload) {
  const stream = new Blob([JSON.stringify(payload)], { type: 'application/json' }).stream();
  const compressed = stream.pipeThrough(new CompressionStream("gzip"));
  const chunks = [];
  for await (const chunk of compressed) {
    chunks.push(chunk);
  }
  return new Blob(chunks);
}

async function download(entry) {
  let name = entry.filename;
  let pathIndex = name.lastIndexOf('/');
  if(pathIndex !== -1) {
    name = name.substring(pathIndex + 1);
  }
  let data = await blobToBase64(await entry.getData(new zip.BlobWriter()));
  return {name, data};
}

async function handleImport() {
  let fileInput = document.getElementById('import-file');
  let fileUrl = document.getElementById('import-url');
  let fileName = document.getElementById('import-name');

  let name = null;
  let entries = [];
  if (fileInput.files.length !== 0) {
    let file = fileInput.files[0];
    let index = file.name.lastIndexOf('.');
    if(index !== -1) {
      name = file.name.substring(0, index);
    } else {
      name = file.name;
    }
    entries = await new zip.ZipReader(new zip.BlobReader(file)).getEntries({});
  }
  else if (fileUrl.value.length !== 0) {
    let url = new URL(fileUrl.value);
    let lastPathIndex = url.pathname.lastIndexOf('/');
    if(lastPathIndex > 0) {
      let final = url.pathname.substring(lastPathIndex + 1);
      let dot = final.lastIndexOf('.');
      name = dot === -1 ? final : final.substring(0, dot);
    }
    entries = await downloadZipFromUrl(url);
  } else {
    throw new Error('Missing URL or file to import');
  }

  if(entries.length === 0) {
    throw new Error('ZIP file is empty');
  }

  const isUtf8 = entries.every(e => e.filenameUTF8);
  if(!isUtf8) {
    throw new Error('ZIP file does not contain UTF-8 files within it');
  }
  const encrypted = entries.some(e => e.encrypted);
  if(encrypted) {
    throw new Error('ZIP file is encrypted and requires password, please use another ZIP');
  }

  const payload = await Promise.all(entries.map(entry => download(entry)));
  let json = await blobToBase64(await compressJson(payload));
  localStorage.setItem('pending_import_file', json);
  fileName.value = decodeURIComponent(name);
}

function showModalAlert(modal, {level, content}) {
  let alert = createAlert({level, content});
  let el = modal.querySelector('h1');
  el.parentNode.insertBefore(alert, el.nextSibling);
}

confirmImport?.addEventListener('click', async (e) => {
  e.preventDefault();
  confirmImport.disabled = true;
  try {
    await handleImport();
  } catch(e) {
    showModalAlert(importModal, {level: 'error', content: e.toString() });
    confirmImport.disabled = false;
    return false;
  }
  confirmImport.disabled = false;
  importForm.requestSubmit(confirmImport);
});

importModal.addEventListener('close', () => {
  document.getElementById('import-file').value = "";
  document.getElementById('import-url').value = "";
});

importButton?.addEventListener('click', () => importModal.showModal());

window.addEventListener('dragenter', (e) => {
  lastDraggedTarget = e.target;
  dropZone.classList.add('dragged');
});

window.addEventListener('dragleave', (e) => {
  if (e.target === lastDraggedTarget || e.target == document) {
    dropZone.classList.remove('dragged');
  }
});

window.addEventListener('dragover', (e) => {
  e.preventDefault();
});

window.addEventListener('drop', (e) => {
  e.preventDefault();
  dropZone.classList.remove('dragged');
  if(e.dataTransfer.files.length === 1 && e.dataTransfer.files[0].name.endsWith('.zip')) {
    document.getElementById('import-file').files = e.dataTransfer.files;
    confirmImport.click();
  }
});
// @license-end
