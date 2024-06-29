/* This file is licensed under AGPL-3.0 */
const uploadButton = document.getElementById('upload-button');
const uploadModal = document.getElementById('upload-modal');
const confirmUpload = document.getElementById('confirm-upload');
const anilistUrl = document.getElementById('anilist-url');
const tmdbUrl = document.getElementById('tmdb-url');

function checkDuplicate() {
  const dir = document.getElementById('directory-name');
  if(anilistUrl !== null) {
    let anilistId = getAnilistId(anilistUrl.value);
    return [...document.querySelectorAll('.entry')].find(e => {
      let id = e.dataset.anilistId;
      let name = e.dataset.name;
      return (id !== null && anilistId !== null && parseInt(id, 10) === anilistId) || (dir !== null && name === dir.value);
    });
  } else if(tmdbUrl !== null) {
    let tmdb = getTmdbId(tmdbUrl.value);
    let tmdbId = tmdb == null ? null : `${tmdb.type}:${tmdb.id}`;
    return [...document.querySelectorAll('.entry')].find(e => {
      let id = e.dataset.tmdbId;
      let name = e.dataset.name;
      return (id !== null && tmdbId !== null && id === tmdbId) || (dir !== null && name === dir.value);
    });
  } else {
    return null;
  }
}

function prefillSearchQuery() {
  let params = new URL(window.location).searchParams;
  let query = params.get('query') ?? params.get('search');
  if(query !== null) {
    filterElement.value = query;
    filterEntries(query);
  }
}

const form = uploadModal.querySelector('form');
confirmUpload.addEventListener('click', (e) => {
  e.preventDefault();
  const dupe = checkDuplicate();
  if(dupe) {
    window.location.href = dupe.querySelector('a').href;
  } else {
    form.requestSubmit(confirmUpload);
  }
});

uploadModal.querySelector('button[formmethod=dialog]').addEventListener('click', (e) => {
  e.preventDefault();
  uploadModal.close();
});

anilistUrl?.addEventListener('input', () => {
  if (anilistUrl.validity.patternMismatch) {
    anilistUrl.setCustomValidity('Invalid AniList URL');
  } else {
    anilistUrl.setCustomValidity('');
  }
});

tmdbUrl?.addEventListener('input', () => {
  if (tmdbUrl.validity.patternMismatch) {
    tmdbUrl.setCustomValidity('Invalid TMDB URL');
  } else {
    tmdbUrl.setCustomValidity('');
  }
});

prefillSearchQuery();
uploadButton?.addEventListener('click', () => uploadModal.showModal());
