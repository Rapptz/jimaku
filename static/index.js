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
      let id = e.getAttribute('data-anilist-id');
      let name = e.getAttribute('data-name');
      return (id !== null && anilistId !== null && parseInt(id, 10) === anilistId) || (dir !== null && name === dir.value);
    });
  } else if(tmdbUrl !== null) {
    let tmdb = getTmdbId(tmdbUrl.value);
    let tmdbId = tmdb == null ? null : `${tmdb.type}:${tmdb.id}`;
    return [...document.querySelectorAll('.entry')].find(e => {
      let id = e.getAttribute('data-tmdb-id');
      let name = e.getAttribute('data-name');
      return (id !== null && tmdbId !== null && id === tmdbId) || (dir !== null && name === dir.value);
    });
  } else {
    return null;
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

uploadButton?.addEventListener('click', () => uploadModal.showModal());
