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

  let js = await callApi(entryForm.action, {
    method: 'POST',
    body: JSON.stringify(payload),
    headers: {
      'content-type': 'application/json',
    },
  });

  if(js !== null) {
    showAlert({level: 'success', content: 'Successfully created entry, redirecting there in 3 seconds...'});
    localStorage.removeItem('pending_import_file');
    await sleep(3000);
    window.location.href = `/entry/${js.entry_id}`;
  }
});

