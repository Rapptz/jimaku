/* This file is licensed under AGPL-3.0 */
const detectChanges = (mutationList, observer) => {
  const el = document.getElementById('security-scheme-Authorization');
  if (el === null) {
    return;
  }
  if (el.type === 'password') {
    el.type = 'text';
  }
}

const observer = new MutationObserver(detectChanges);
observer.observe(document.body, {childList: true, subtree: true})
