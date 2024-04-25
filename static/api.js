// @license magnet:?xt=urn:btih:0b31508aeb0634b347b8270c7bee4d411b5d4109&dn=agpl-3.0.txt AGPL-v3-or-Later
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
// @license-end
