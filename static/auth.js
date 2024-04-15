const username = document.getElementById('username');

async function invalidateToken(button, token) {
  await fetch('/account/invalidate', {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
    },
    body: JSON.stringify({
      session_id: token,
    })
  });
  let element = button.parentElement;
  let parent = element.parentElement;
  console.log(element, parent);
  parent.removeChild(element);
  if(parent.childElementCount === 0) {
    window.location.reload();
  }
}

username?.addEventListener('input', () => {
  if (username.validity.patternMismatch) {
    username.setCustomValidity("Must be all lowercase letters, numbers, or .-_ characters");
  } else {
    username.setCustomValidity("");
  }
});

document.querySelectorAll('.password-icon').forEach(el => {
  el.addEventListener('click', () => {
    let input = el.previousElementSibling;
    let img = el.firstElementChild;
    if(input.type === 'password') {
      input.type = 'text';
      img.src = '/static/visibility_off.svg';
    } else {
      input.type = 'password';
      img.src = '/static/visibility.svg';
    }
  })
})

document.getElementById('change-password')?.addEventListener('click', () => {
  document.getElementById('change-password-modal').showModal();
});
document.querySelector('#change-password-modal .button[formmethod="dialog"]')?.addEventListener('click', () => {
  document.getElementById('change-password-modal').close();
});
const toggleEditor = document.getElementById('toggle-editor');
toggleEditor?.addEventListener('click', async () => {
  let editor = toggleEditor.getAttribute('data-editor') == 'false';
  let resp = await callApi(toggleEditor.getAttribute('data-endpoint'), {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
    },
    body: JSON.stringify({editor})
  });
  if (resp !== null) {
    let content = editor ? 'made user an editor' : 'removed editor flag from user';
    showAlert({level: 'success', content: `Successfully ${content}.`});
    await sleep(2000);
    window.location.reload();
  }
})

document.getElementById('session-description')?.setAttribute('value', deviceDescription());

document.querySelectorAll('.created[data-timestamp]').forEach(el => {
  let seconds = parseInt(el.dataset.timestamp, 10);
  el.textContent = formatRelative(seconds);
});

document.querySelectorAll('.invalidate[data-token]').forEach(el => {
  const token = el.dataset.token;
  el.removeAttribute('data-token');
  el.addEventListener('click', () => invalidateToken(el, token));
});
