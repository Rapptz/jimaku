// @license magnet:?xt=urn:btih:0b31508aeb0634b347b8270c7bee4d411b5d4109&dn=agpl-3.0.txt AGPL-v3-or-Later

async function processTrashRequest(action) {
  let files = getSelectedFiles().map(e => e.textContent);
  if (files.length === 0) {
    return;
  }

  let payload = {files, action};

  let js = await callApi(`/admin/trash`, {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
    },
    body: JSON.stringify(payload)
  });

  if(js === null) {
    return;
  }

  let total = js.success + js.failed;
  if (js.success != 0) {
    showAlert({level: 'success', content: `Successfully ${action}d ${js.success}/${total} files, refreshing in 3 seconds`});
    await sleep(3000);
    window.location.reload();
  } else {
    showAlert({level: 'error', content: `Failed to ${action} ${total} files.`});
  }
}

document.getElementById('trash-files').addEventListener('click', () => processTrashRequest('delete'));
document.getElementById('restore-files').addEventListener('click', () => processTrashRequest('restore'));

// @license-end
