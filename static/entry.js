/* This file is licensed under AGPL-3.0 */
const editModal = document.getElementById('edit-entry-modal');
const editButton = document.getElementById('edit-entry');

const uploadForm = document.getElementById('upload-form');
const uploadInput = document.getElementById('upload-file-input');

const updateInfo = document.getElementById('update-info');

const dropZone = document.getElementById('file-upload-drop-zone');
let lastDraggedTarget = null;
const counterRenameRegex = /\$\{(?:(?:(start|increment|padding)=(\d+))(?:,\s*)?(?:(start|increment|padding)=(\d+))?)?(?:,\s*)?(?:(start|increment|padding)=(\d+))?\}/ig;

class RenameOptions {
  constructor(parent) {
    this.search = document.getElementById('rename-search').value;
    this.repl = document.getElementById('rename-replace').value;
    this.isEmpty = this.search.length === 0;
    this.isRegex = document.getElementById('rename-use-regex').checked;
    this.matchAll = document.getElementById('rename-match-all').checked;
    this.caseSensitive = document.getElementById('rename-case-sensitive').checked;
    this.applyTo = document.getElementById('rename-apply').value;
    this.caseTransform = document.getElementById('rename-text-formatting').value;
    this.counterInfo = {};
    this.currentIndex = 0;
    this.repl = this.repl.replaceAll(counterRenameRegex, (m, p1, p2, p3, p4, p5, p6, offset) => this.parseCounterInfo(p1, p2, p3, p4, p5, p6, offset));

    let flags = '';
    if(!this.caseSensitive) flags += 'i';
    if(this.matchAll) flags += 'g';

    if(this.isRegex) {
      this.search = new RegExp(this.search, flags);
    } else {
      this.search = new RegExp(escapeRegex(this.search), flags);
    }

    this.files = parent.getSelectedFiles().map(e => e.textContent);
    this.renamed = this.files.map(f => this.rename(f));
  }

  parseCounterInfo(p1, p2, p3, p4, p5, p6, offset) {
    let obj = {};
    if(p1 != null) obj[p1] = p2 ?? null;
    if(p3 != null) obj[p3] = p4 ?? null;
    if(p5 != null) obj[p5] = p6 ?? null;
    obj.increment = obj.increment != null ? parseInt(obj.increment, 10) : 1;
    obj.start = obj.start != null ? parseInt(obj.start, 10) : 1;
    obj.padding = obj.padding != null ? parseInt(obj.padding, 10) : 0;
    this.counterInfo[offset] = obj;
    return `{__internal_jimaku_counter:${offset}}`;
  }

  replaceCounterInfo(counter) {
    let info = this.counterInfo[counter];
    let result = info.start + this.currentIndex * info.increment;
    if(info.padding !== 0) {
      return result.toString().padStart(info.padding, '0');
    }
    return result.toString();
  }

  updateTable(tbody) {
    tbody.innerHTML = '';
    for(let i = 0; i < this.files.length; ++i) {
      let original = this.files[i];
      let renamed = this.renamed[i];
      let el = html('tr',
        html('td', {dataset: {th: 'Original'}}, original),
        html('td', {dataset: {th: 'Renamed'}}, original != renamed ? [renamed, {class: 'changed'}] : null),
      );
      tbody.appendChild(el);
    }
  }

  toJSON() {
    return this.files.map((e, i) => { return {from: e, to: this.renamed[i]}; }).filter(obj => obj.from !== obj.to);
  }

  replace(s) {
    if(this.isEmpty) return s;
    let initial = this.matchAll ? s.replaceAll(this.search, this.repl).trim() : s.replace(this.search, this.repl).trim();
    if(this.counterInfo.length === 0) {
      return initial;
    }
    let final = initial.replaceAll(/{__internal_jimaku_counter:(\d+)}/g, (m, p1) => this.replaceCounterInfo(p1));
    if(initial != s) {
      this.currentIndex += 1;
    }
    return final;
  }

  transformCase(s) {
    if(this.caseTransform === 'none') {
      return s;
    } else if(this.caseTransform === 'lower') {
      return s.toLowerCase();
    } else {
      return s.toUpperCase();
    }
  }

  rename(filename) {
    if(this.applyTo === 'file') {
      let idx = filename.lastIndexOf('.');
      if(idx !== -1) {
        let changed = this.transformCase(this.replace(filename.substring(0, idx)));
        return changed + filename.substring(idx);
      }
    } else if(this.applyTo === 'ext') {
      let idx = filename.lastIndexOf('.');
      if(idx !== -1) {
        let changed = this.transformCase(this.replace(filename.substring(idx + 1)));
        if(changed.length !== 0) {
          return filename.substring(0, idx) + '.' + changed;
        } else {
          return filename.substring(0, idx);
        }
      }
    }
    return this.transformCase(this.replace(filename));
  }
}

class BulkFilesOperations {
  static checkedSelector = '.entry:not(.hidden) > .file-bulk > input[type="checkbox"]';

  constructor(
    table,
    entryId,
    {
      deleteFiles = null,
      deleteModal = null,
      renameFiles = null,
      renameModal = null,
      moveFiles = null,
      moveModal = null,
      reportFiles = null,
      reportModal = null,
      downloadFiles = null,
      totalFileCount = null,
      selectedFileCount = null,
    } = {},
  ) {
    this.parent = table;
    this.entryId = entryId;
    this.currentRenameOption = null;
    this.checkboxAnchor = null;
    this.bulkCheck = table?.querySelector('.bulk-check');
    this.deleteFilesButton = deleteFiles;
    this.renameFilesButton = renameFiles;
    this.moveFilesButton = moveFiles;
    this.reportFilesButton = reportFiles;
    this.downloadFilesButton = downloadFiles;
    this.totalFileCount = totalFileCount;
    this.selectedFileCount = selectedFileCount;
    this.reportModal = reportModal;
    this.deleteModal = deleteModal;
    this.renameModal = renameModal;
    this.moveModal = moveModal;
    // These require hardcoded IDs since forms tend have IDs
    this.confirmDeleteButton = document.getElementById('confirm-delete');
    this.confirmReportButton = document.getElementById('confirm-report');
    this.confirmRenameButton = document.getElementById('confirm-rename')
    this.confirmMoveButton = document.getElementById('confirm-move');

    this.bulkCheck?.addEventListener('click', () => this.processBulkCheck());
    this.renameModal?.querySelector('form')?.addEventListener('input', debounced(() => this.updateRenameOptions(), 150));

    this.confirmMoveButton?.addEventListener('click', (e) => {
      e.preventDefault();
      this.moveFiles();
    });
    this.confirmDeleteButton?.addEventListener('click', (e) => {
      e.preventDefault();
      let form = this.deleteModal?.querySelector('form');
      if(form?.reportValidity()) {
        this.deleteFiles();
        form.reset();
      }
    });
    this.confirmReportButton?.addEventListener('click', (e) => {
      e.preventDefault();
      let form = this.reportModal?.querySelector('form');
      if(form?.reportValidity()) {
        this.reportFiles();
        form.reset();
      }
    });
    this.confirmRenameButton?.addEventListener('click', (e) => {
      e.preventDefault();
      this.renameFiles();
    });
    this.reportModal?.querySelector('button[formmethod=dialog]')?.addEventListener('click', (e) => this.closeModal(e, this.reportModal));
    this.deleteModal?.querySelector('button[formmethod=dialog]')?.addEventListener('click', (e) => this.closeModal(e, this.deleteModal));
    this.renameModal?.querySelector('button[formmethod=dialog]')?.addEventListener('click', (e) => this.closeModal(e, this.renameModal));
    this.moveModal?.querySelector('button[formmethod=dialog]')?.addEventListener('click', (e) => this.closeModal(e, this.moveModal));

    this.deleteFilesButton?.addEventListener('click', () => this.showConfirmFileModal(this.deleteModal));
    this.reportFilesButton?.addEventListener('click', () => this.showConfirmFileModal(this.renameModal));
    this.moveFilesButton?.addEventListener('click', () => this.moveModal?.showModal());
    this.downloadFilesButton?.addEventListener('click', () => this.downloadFiles());
    this.renameFilesButton?.addEventListener('click', () => this.openRenameModal());

    document.addEventListener('entries-filtered', () => this.setCheckboxState());
    this.parent?.querySelectorAll('.file-bulk > input[type="checkbox"]').forEach(ch => {
      ch.addEventListener('click', (e) => {
        this.handleCheckboxClick(e);
        this.setCheckboxState();
      });
    });
  }

  closeModal(event, modal) {
    event.preventDefault();
    modal.close();
  }

  showConfirmFileModal(modal) {
    if(!modal) return;
    let files = this.getSelectedFiles();
    let span = modal.querySelector('span');
    span.textContent = files.length === 1 ? '1 file' : files.length === 0 ? `the entire entry` : `${files.length} files`;
    modal.showModal();
  }

  processBulkCheck() {
    let indeterminate = this.bulkCheck.getAttribute('tribool') === 'yes';
    let checked = indeterminate ? false : this.bulkCheck.checked;
    let selected = [...this.parent.querySelectorAll(this.constructor.checkedSelector)];
    for(const ch of selected) {
      ch.checked = checked;
    }

    this.disableButtons(!checked);
    this.updateFileCounts(checked ? selected.length : 0);
    if (indeterminate) {
      this.bulkCheck.checked = false;
      this.bulkCheck.indeterminate = false;
      this.bulkCheck.removeAttribute('tribool');
    }
  }

  handleCheckboxClick(e) {
    if(e.ctrlKey) {
      return;
    }
    if(!e.shiftKey) {
      this.checkboxAnchor = e.target;
      return;
    }
    let activeCheckboxes = [...this.parent.querySelectorAll(this.constructor.checkedSelector)];
    let startIndex = activeCheckboxes.indexOf(this.checkboxAnchor);
    let endIndex = activeCheckboxes.indexOf(e.target);
    if(startIndex == endIndex || startIndex == -1 || endIndex == -1) {
      return;
    }

    if(startIndex > endIndex) {
      let temp = endIndex;
      endIndex = startIndex;
      startIndex = temp;
    }

    for(let i = startIndex; i <= endIndex; ++i) {
      let cb = activeCheckboxes[i];
      cb.checked = true;
    }
  }

  getSelectedFiles() {
    return [...this.parent.querySelectorAll(this.constructor.checkedSelector + ':checked')].map(e => {
      return e.parentElement.parentElement.querySelector('.file-name');
    });
  }

  updateFileCounts(checked = null) {
    if(checked === null) {
      let checkboxes = [...this.parent.querySelectorAll(this.constructor.checkedSelector)];
      checked = checkboxes.reduce((prev, el) => prev + el.checked, 0);
    }
    if(this.selectedFileCount) {
      this.selectedFileCount.classList.toggle('hidden', checked === 0);
      this.selectedFileCount.textContent = `${checked} file${checked !== 1 ? 's' : ''} selected`;
    }

    if(this.totalFileCount) {
      let total = [...this.parent.querySelectorAll('.entry:not(.hidden)')].length;
      this.totalFileCount.textContent = `${total} file${total !== 1 ? 's' : ''}`;
    }
  }

  removeCheckedFiles() {
    this.parent.querySelectorAll(this.constructor.checkedSelector + ':checked').forEach(e => {
      let parent = e.parentElement.parentElement;
      return parent.parentElement.removeChild(parent);
    });
    this.setCheckboxState();
  }

  disableButtons(disabled) {
    if(this.moveFilesButton) this.moveFilesButton.disabled = disabled;
    if(this.renameFilesButton) this.renameFilesButton.disabled = disabled;
    if(this.downloadFilesButton) this.downloadFilesButton.disabled = disabled;
  }

  setCheckboxState() {
    let checkboxes = [...this.parent.querySelectorAll(this.constructor.checkedSelector)];
    let checked = checkboxes.reduce((prev, el) => prev + el.checked, 0);
    let nothingChecked = checked === 0;
    this.disableButtons(nothingChecked);
    this.updateFileCounts(checked);

    if(nothingChecked) {
      this.bulkCheck.checked = false;
      this.bulkCheck.indeterminate = false;
      this.bulkCheck.removeAttribute('tribool');
    }
    else if(checked === checkboxes.length) {
      this.bulkCheck.indeterminate = false;
      this.bulkCheck.removeAttribute('tribool');
      this.bulkCheck.checked = true;
    } else {
      this.bulkCheck.indeterminate = true;
      this.bulkCheck.setAttribute('tribool', 'yes');
      this.bulkCheck.checked = false;
    }
  }

  async downloadFiles() {
    let files = this.getSelectedFiles();
    if (files.length === 0) {
      return;
    }
    if (files.length === 1) {
      files[0].click();
      return;
    }
    files = files.map(e => e.textContent);
    let payload = {files};
    let resp = await fetch(`/entry/${this.entryId}/bulk`, {
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
      let blob = await resp.blob();
      const a = html('a.hidden', {href: URL.createObjectURL(blob), download: resp.headers.get("x-jimaku-filename")});
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
    }
    this.bulkCheck.click();
  }

  async deleteFiles() {
    let files = this.getSelectedFiles().map(e => e.textContent);
    let payload = {files};
    if (files.length === 0) {
      payload.delete_parent = true;
    }
    payload.reason = document.getElementById('delete-reason').value || null;
    let js = await callApi(`/entry/${this.entryId}`, {
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
    this.deleteModal.close();
    this.removeCheckedFiles();
    if(payload.delete_parent) {
      await sleep(3000);
      window.location.href = '/';
    }
  }

  async reportFiles() {
    let files = this.getSelectedFiles().map(e => e.textContent);
    let payload = {files};
    payload.reason = document.getElementById('report-reason').value;
    let js = await callApi(`/entry/${this.entryId}/report`, {
      method: 'POST',
      headers: {
        'content-type': 'application/json',
      },
      body: JSON.stringify(payload)
    });

    if(js === null) {
      return;
    }

    if(files.length === 0) {
      showAlert({level: 'success', content: 'Successfully reported entry, editors and administrators have been notified.'});
    } else {
      let total = files.length;
      showAlert({level: 'success', content: `Successfully reported ${total} file${total == 1 ? "s" : ""}, editors and administrators have been notified.`});
    }
    this.reportModal.close();
  }

  async moveFiles() {
    let files = this.getSelectedFiles().map(e => e.textContent);
    if (files.length === 0) {
      return;
    }

    let params = new URLSearchParams();
    let payload = {files};
    let destinationId = parseInt(document.getElementById('move-to-entry-id')?.value, 10);
    if (!Number.isNaN(destinationId)) {
      payload.entry_id = destinationId;
    } else {
      let anilistId = getAnilistId(document.getElementById('anilist-url')?.value);
      if (anilistId !== null) {
        payload.anilist_id = anilistId;
        params.append('anilist_id', anilistId);
      }
      let tmdbId = getTmdbId(document.getElementById('tmdb-url')?.value);
      if (tmdbId !== null) {
        payload.tmdb = `${tmdbId.type}:${tmdbId.id}`;
        payload.anime = false;
        params.append('tmdb_id', payload.tmdb);
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
    }

    if(Object.keys(payload).length === 1) {
      showModalAlert(this.moveModal, {level: 'error', content: 'Either a name, AniList URL, or TMDB URL is required'});
      return;
    }

    let js = await callApi(`/entry/${this.entryId}/move`, {
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
    showModalAlert(this.moveModal, {level: 'success', content: `Successfully moved ${js.success}/${total} files, redirecting to folder in 5 seconds...`});
    await sleep(5000);
    window.location.href = `/entry/${js.entry_id}`;
  }

  updateRenameOptions() {
    let search = document.getElementById('rename-search');
    try {
      this.currentRenameOption = new RenameOptions(this);
    } catch(e) {
      if (e instanceof SyntaxError) {
        search.setCustomValidity('Invalid regex provided');
      }
      return;
    }
    search.setCustomValidity('');
    this.currentRenameOption.updateTable(this.renameModal.querySelector('table#renamed-files > tbody'));
  }

  openRenameModal() {
    let form = this.renameModal.querySelector('form');
    form.reset();
    this.currentRenameOption = new RenameOptions(this);
    this.currentRenameOption.updateTable(this.renameModal.querySelector('table#renamed-files > tbody'));
    this.renameModal.showModal();
  }

  async renameFiles() {
    if(this.currentRenameOption === null) {
      return;
    }
    let payload = this.currentRenameOption.toJSON();
    if(payload.length === 0) {
      return;
    }

    let js = await callApi(`/entry/${entryId}/rename`, {
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
    showModalAlert(this.renameModal, {level: 'success', content: `Successfully renamed ${js.success}/${total} files, refreshing in 3 seconds...`});
    await sleep(3000);
    window.location.reload();
  }
}

const query = `
query ($id: Int) {
  Media (id: $id, type: ANIME) {
    id
    title {
      romaji
      english
      native
    }
    isAdult
    format
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

const fileExtension = (name) => name.slice((name.lastIndexOf('.') - 1 >>> 0) + 2);
const allowedExtensions = ["srt", "ssa", "ass", "zip", "sub", "sup", "idx", "7z"];

function filterValidFileList(files) {
  let filtered = Array.from(files).filter(f => allowedExtensions.includes(fileExtension(f.name)));
  const dt = new DataTransfer();
  filtered.forEach(f => dt.items.add(f));
  return dt.files;
}

function showModalAlert(modal, {level, content}) {
  if(modal) {
    let alert = createAlert({level, content});
    let el = modal.querySelector('h1');
    el.parentNode.insertBefore(alert, el.nextSibling);
  } else {
    showAlert({level, content});
  }
}

function modalAlertHook(modal) {
  let el = modal.querySelector('h1');
  return (e) => el.parentNode.insertBefore(e, el.nextSibling);
}

function updateEntryFields(titles, adult, movie) {
  if (titles?.romaji) {
    document.getElementById('entry-name').value = titles.romaji;
  }
  if (titles?.native) {
    document.getElementById('entry-japanese-name').value = titles.native;
  }
  if (titles?.english) {
    document.getElementById('entry-english-name').value = titles.english;
  }
  document.getElementById('entry-movie').checked = movie;
  document.getElementById('entry-adult').checked = adult;
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
    let media = js?.data?.Media;
    if(media) {
      updateEntryFields(media.title, media.isAdult, media.format === 'MOVIE');
    }
    showModalAlert(editModal, {level: 'success', content: `Updated info from AniList`});
  } else {
    showModalAlert(editModal, {level: 'error', content: `AniList returned ${response.status}`})
  }
};

async function refreshTmdbNames(id) {
  let param = encodeURIComponent(`${id.type}:${id.id}`);
  let response = await fetch(`/entry/tmdb?id=${param}`);
  if (response.ok) {
    let js = await response.json();
    if(js) {
      updateEntryFields(js.title, js.adult, js.movie);
    }
    showModalAlert(editModal, {level: 'success', content: `Updated info from TMDB`});
  } else {
    showModalAlert(editModal, {level: 'error', content: `API returned ${response.status}`})
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
  let div = document.getElementById('relations');
  if (div === null) return;
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
  div.innerHTML = '<span>Related</span>';
  for (const entry of js) {
    let el = html('a.relation.file-name', getPreferredNameForEntry(entry), {
      href: `/entry/${entry.id}`,
      dataset: {
        name: entry.name,
        japaneseName: entry.japanese_name !== null ? entry.japanese_name : undefined,
        englishName: entry.english_name !== null ? entry.english_name : undefined,
      }
    });
    div.appendChild(el);
  }
}

editButton?.addEventListener('click', () => editModal?.showModal());
updateInfo?.addEventListener('click', async (e) => {
  e.preventDefault();
  let text = document.getElementById('entry-anilist-id').value;
  let id = parseInt(text, 10);
  if(Number.isNaN(id)) {
    id = getAnilistId(text);
  }
  if(id) {
    await getAnimeInfo(id)
  } else {
    let tmdb = getTmdbId(document.getElementById('entry-tmdb-url').value);
    if(tmdb === null) {
      showModalAlert(editModal, {level: 'error', content: 'Missing or invalid AniList ID or TMDB URL'});
    } else {
      await refreshTmdbNames(tmdb);
    }
  }
});

populateAnimeRelations();
uploadInput?.addEventListener('change', () => {
  uploadForm.submit();
});

var __bulk = new BulkFilesOperations(document.querySelector('.files'), entryId, {
  deleteFiles: document.getElementById('delete-files'),
  deleteModal: document.getElementById('confirm-delete-modal'),
  renameFiles: document.getElementById('rename-files'),
  renameModal: document.getElementById('rename-entries-modal'),
  moveFiles: document.getElementById('move-files'),
  moveModal: document.getElementById('move-entries-modal'),
  reportFiles: document.getElementById('report-files'),
  reportModal: document.getElementById('confirm-report-modal'),
  downloadFiles: document.getElementById('download-files'),
  totalFileCount: document.getElementById('total-file-count'),
  selectedFileCount: document.getElementById('selected-file-count'),
});

if (uploadInput !== null) {
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
    let files = filterValidFileList(e.dataTransfer.files);
    if(files.length > 0) {
      uploadInput.files = files;
      uploadForm.submit();
    }
  });
}
