const editModal = document.getElementById('edit-entry-modal');
const moveModal = document.getElementById('move-entries-modal');
const deleteModal = document.getElementById('confirm-delete-modal');

const editButton = document.getElementById('edit-entry');
const deleteFilesButton = document.getElementById('delete-files');
const downloadFilesButton = document.getElementById('download-files');
const moveFilesButton = document.getElementById('move-files');

const uploadForm = document.getElementById('upload-form');
const uploadInput = document.getElementById('upload-file-input');

const anilistSync = document.getElementById('anilist-sync');

const checkedSelector = '.entry:not(.hidden) > .file-bulk > input[type="checkbox"]';
const query = `
query ($id: Int) {
  Media (id: $id, type: ANIME) {
    id
    title {
      romaji
      english
      native
    }
  }
}`;

const relationQuery = `
query ($id: Int) {
  Media(id: $id) {
    relations {
      edges {
        relationType
        node {
          id
          type
          title {
            romaji
            native
            english
          }
        }
      }
    }
  }
}
`;

function getSelectedFiles() {
  return [...document.querySelectorAll(checkedSelector + ':checked')].map(e => {
    return e.parentElement.parentElement.querySelector('.file-name').textContent;
  });
}

function removeCheckedFiles() {
  document.querySelectorAll(checkedSelector + ':checked').forEach(e => {
    let parent = e.parentElement.parentElement;
    return parent.parentElement.removeChild(parent);
  });
  setCheckboxState();
}

const disableButtons = (disabled) => {
  if(moveFilesButton) moveFilesButton.disabled = disabled;
  downloadFilesButton.disabled = disabled;
}

function showModalAlert(modal, {level, content}) {
  let alert = createAlert({level, content});
  let el = modal.querySelector('h1');
  el.parentNode.insertBefore(alert, el.nextSibling);
}

function modalAlertHook(modal) {
  let el = modal.querySelector('h1');
  return (e) => el.parentNode.insertBefore(e, el.nextSibling);
}

async function getAnimeInfo(id) {
  let response = await fetch('https://graphql.anilist.co', {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
    },
    body: JSON.stringify({
      query: query,
      variables: { id }
    })
  });
  if (response.ok) {
    let js = await response.json();
    let titles = js?.data?.Media?.title;
    if (titles?.romaji) {
      document.getElementById('entry-name').value = titles.romaji;
    }
    if (titles?.native) {
      document.getElementById('entry-japanese-name').value = titles.native;
    }
    if (titles?.english) {
      document.getElementById('entry-english-name').value = titles.english;
    }
    showModalAlert(editModal, {level: 'success', content: `Updated names from AniList`});
  } else {
    showModalAlert(editModal, {level: 'error', content: `AniList returned ${response.status}`})
  }
};

async function getAnimeRelationIds(id) {
  let response = await fetch('https://graphql.anilist.co', {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
    },
    body: JSON.stringify({
      query: relationQuery,
      variables: { id }
    })
  });
  if (response.ok) {
    let js = await response.json();
    let nodes = js?.data?.Media?.relations?.edges ?? [];
    return nodes.map(e => e.node.id);
  } else {
    console.log(`AniList returned ${response.status}`);
    return [];
  }
}

async function populateAnimeRelations() {
  if (entryData.anilist_id == null) {
    return;
  }
  let ids = await getAnimeRelationIds(entryData.anilist_id);
  if (ids.length === 0) {
    return;
  }

  let response = await fetch('/entry/relations', {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
    },
    body: JSON.stringify({
      anilist_ids: ids
    })
  });
  if (!response.ok) {
    console.log(`Server returned ${response.status} for relations`);
    return;
  }
  let js = await response.json();
  if(js.length === 0) return;
  let div = document.getElementById('relations');
  div.innerHTML = '<span>Related</span>';
  for (const entry of js) {
    let el = document.createElement('a');
    el.href = `/entry/${entry.id}`
    el.textContent = getPreferredNameForEntry(entry);
    el.classList.add('relation');
    el.classList.add('file-name');
    el.setAttribute('data-name', entry.name);
    if (entry.japanese_name !== null) {
      el.setAttribute('data-japanese-name', entry.japanese_name);
    }
    if (entry.english_name !== null) {
      el.setAttribute('data-english-name', entry.english_name);
    }
    div.appendChild(el);
  }
}

async function downloadFiles() {
  let files = getSelectedFiles();
  if (files.length === 0) {
    return;
  }
  let payload = {files};
  let js = await fetch(`/entry/${entryId}/bulk`, {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
    },
    body: JSON.stringify(payload)
  });

  if(!resp.ok) {
    let content = `Server responded with status code ${resp.status}`;
    if(resp.headers.get('content-type') === 'application/json') {
      let js = await resp.json();
      content = js.error;
    }
    showAlert({level: 'error', content});
  } else {
    const a = document.createElement('a');
    let blob = await resp.blob();
    a.href = URL.createObjectURL(blob);
    a.download = resp.headers.get("x-jimaku-filename");
    a.classList.add("hidden");
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
  }
  bulkCheck.click();
}

editButton?.addEventListener('click', () => editModal?.showModal());
anilistSync?.addEventListener('click', async (e) => {
  e.preventDefault();
  let id = parseInt(document.getElementById('entry-anilist-id').value, 10);
  if(id) {
    await getAnimeInfo(id)
  } else {
    showModalAlert(editModal, {level: 'error', content: 'Missing or invalid AniList ID'});
  }
});

const bulkCheck = document.getElementById('bulk-check');
bulkCheck.addEventListener('click', () => {
  let indeterminate = bulkCheck.getAttribute('tribool') === 'yes';
  let checked = indeterminate ? false : bulkCheck.checked;
  document.querySelectorAll(checkedSelector).forEach(ch => {
    ch.checked = checked;
  });

  disableButtons(!checked);
  if (indeterminate) {
    bulkCheck.checked = false;
    bulkCheck.indeterminate = false;
    bulkCheck.removeAttribute('tribool');
  }
});

function setCheckboxState() {
  let checkboxes = [...document.querySelectorAll(checkedSelector)];
  let checked = checkboxes.reduce((prev, el) => prev + el.checked, 0);
  let nothingChecked = checked === 0;
  disableButtons(nothingChecked);

  if(nothingChecked) {
    bulkCheck.checked = false;
    bulkCheck.indeterminate = false;
    bulkCheck.removeAttribute('tribool');
  }
  else if(checked === checkboxes.length) {
    bulkCheck.indeterminate = false;
    bulkCheck.removeAttribute('tribool');
    bulkCheck.checked = true;
  } else {
    bulkCheck.indeterminate = true;
    bulkCheck.setAttribute('tribool', 'yes');
    bulkCheck.checked = false;
  }
}

async function deleteFiles() {
  let files = getSelectedFiles();
  let payload = {files};
  if (files.length === 0) {
    payload.delete_parent = true;
  }
  let js = await callApi(`/entry/${entryId}`, {
    method: 'DELETE',
    headers: {
      'content-type': 'application/json',
    },
    body: JSON.stringify(payload)
  });

  if(js === null) {
    return;
  }

  if(payload.delete_parent) {
    showAlert({level: 'success', content: 'Successfully deleted entry, redirecting you back home...'});
  } else {
    let total = js.success + js.failed;
    showAlert({level: 'success', content: `Successfully deleted ${js.success}/${total} file${total == 1 ? "s" : ""}`});
  }
  deleteModal.close();
  removeCheckedFiles();
  if(payload.delete_parent) {
    await sleep(3000);
    window.location.href = '/';
  }
}

async function moveFiles() {
  let files = getSelectedFiles();
  if (files.length === 0) {
    return;
  }

  let params = new URLSearchParams();
  let payload = {files};
  let anilistId = getAnilistId(document.getElementById('anilist-url')?.value);
  if (anilistId !== null) {
    payload.anilist_id = anilistId;
    params.append('anilist_id', anilistId);
  }
  let name = document.getElementById('directory-name').value;
  if(name) {
    payload.name = name;
    params.append('name', name);
  }
  let resp = await fetch('/entry/search?' + params);
  if(resp.ok) {
    let js = await resp.json();
    payload.entry_id = js.entry_id;
  }
  let js = await callApi(`/entry/${entryId}/move`, {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
    },
    body: JSON.stringify(payload)
  }, modalAlertHook);

  if(js === null) {
    return;
  }

  let total = js.success + js.failed;
  showModalAlert(moveModal, {level: 'success', content: `Successfully moved ${js.success}/${total} files, redirecting to folder in 5 seconds...`});
  await sleep(5000);
  window.location.href = `/entry/${js.entry_id}`;
}

populateAnimeRelations();
deleteFilesButton?.addEventListener('click', () => {
  let files = getSelectedFiles();
  let span = deleteModal.querySelector('span');
  span.textContent = files.length === 1 ? '1 file' : files.length === 0 ? `the entire entry` : `${files.length} files`;
  deleteModal.showModal();
});
document.getElementById('confirm-move')?.addEventListener('click', (e) => {
  e.preventDefault();
  moveFiles();
});
document.getElementById('confirm-delete')?.addEventListener('click', (e) => {
  e.preventDefault();
  deleteFiles();
});
moveFilesButton?.addEventListener('click', () => moveModal?.showModal());
document.getElementById('clear-search-filter').addEventListener('click', setCheckboxState);
filterElement.addEventListener('input', setCheckboxState);
document.querySelectorAll('.file-bulk > input[type="checkbox"]').forEach(ch => {
  ch.addEventListener('click', setCheckboxState);
});

uploadInput?.addEventListener('change', () => {
  uploadForm.submit();
});
downloadFilesButton.addEventListener('click', downloadFiles);
