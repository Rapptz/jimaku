/**
 *
 * @licstart  The following is the entire license notice for the
 *  JavaScript code in this page.
 *
 * Copyright (C) 2024  Rapptz <rapptz at gmail dot com>
 *
 * The JavaScript code in this page is free software: you can
 * redistribute it and/or modify it under the terms of the GNU Affero
 * General Public License as published by the Free Software Foundation,
 * either version 3 of the License, or (at your option) any later version.
 *
 * The code is distributed WITHOUT ANY WARRANTY; without even the
 * implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
 * See the GNU Affero General Public License for more details.
 *
 * As additional permission under GNU Affero General Public License
 * section 7, you may distribute non-source (e.g., minimized or compacted)
 * forms of that code without the copy of the GNU Affero General Public
 * License normally required by section 4, provided you include this
 * license notice and a URL through which recipients can access the
 * Corresponding Source.
 *
 * @licend  The above is the entire license notice
 *  for the JavaScript code in this page.
 */

const confirmImport = document.getElementById('confirm-import');
const entryForm = document.querySelector('form');

function base64ToBlob(data) {
  const decoded = atob(data);
  const bytes = new Uint8Array(new ArrayBuffer(decoded.length));
  for(let i = 0; i < decoded.length; ++i) {
    bytes[i] = decoded.charCodeAt(i);
  }
  return new Blob([bytes]);
}

async function getImportedFiles() {
  let importedFiles = localStorage.getItem('pending_import_file');
  if(importedFiles === null) {
    return [];
  }
  let blob = base64ToBlob(importedFiles);
  const stream = blob.stream().pipeThrough(new DecompressionStream("gzip"));
  const chunks = [];
  for await (const chunk of stream) {
    chunks.push(chunk);
  }
  const jsonBlob = new Blob(chunks);
  return JSON.parse(await jsonBlob.text());
}

confirmImport.addEventListener('click', async (e) => {
  e.preventDefault();
  confirmImport.disabled = true;

  let data = new FormData(entryForm);
  let anilistId = data.get('anilist_id');
  if(anilistId !== null && !anilistId.startsWith('http')) {
    anilistId = parseInt(anilistId, 10);
  }
  let payload = {
    name: data.get('name'),
    japanese_name: data.get('japanese_name'),
    english_name: data.get('english_name'),
    notes: data.get('notes'),
    anilist_id: anilistId,
    tmdb_url: data.get('tmdb_url'),
    low_quality: data.get('low_quality') !== null,
    movie: data.get('movie') !== null,
    adult: data.get('adult') !== null,
    files: await getImportedFiles(),
  };

  let js = null;
  try {
    js = await callApi(entryForm.action, {
      method: 'POST',
      body: JSON.stringify(payload),
      headers: {
        'content-type': 'application/json',
      },
    });
  } catch(e) {
    showAlert({level: 'error', content: `Failed to call API: ${e}`});
  }

  if(js !== null) {
    showAlert({level: 'success', content: 'Successfully created entry, redirecting there in 3 seconds...'});
    localStorage.removeItem('pending_import_file');
    await sleep(3000);
    window.location.href = `/entry/${js.entry_id}`;
  } else {
    confirmImport.disabled = false;
  }
});

