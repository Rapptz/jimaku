const uploadButton = document.getElementById('upload-button');
const uploadModal = document.getElementById('upload-modal');

function checkDuplicate() {
  const dir = document.getElementById('directory-name');
  let anilistId = getAnilistId(anilistUrl.value);
  return [...document.querySelectorAll('.entry')].find(e => {
    let id = e.getAttribute('data-anilist-id');
    let name = e.getAttribute('data-name');
    return (id !== null && anilistId !== null && parseInt(id, 10) === anilistId) || (dir !== null && name === dir.value);
  });
}

const anilistUrl = document.getElementById('anilist-url');
const form = uploadModal.querySelector('form');
form.addEventListener('submit', (e) => {
  e.preventDefault();
  const dupe = checkDuplicate();
  if (dupe !== null) {
    window.location.href = dupe.querySelector('a').href;
    return false;
  }
  return true;
});

uploadModal.querySelector('button[formmethod=dialog]').addEventListener('click', (e) => {
  e.preventDefault();
  uploadModal.close();
});

anilistUrl.addEventListener('input', () => {
  if (anilistUrl.validity.patternMismatch) {
    anilistUrl.setCustomValidity('Invalid AniList URL');
  } else {
    anilistUrl.setCustomValidity('');
  }
});

uploadButton?.addEventListener('click', () => uploadModal.showModal());
