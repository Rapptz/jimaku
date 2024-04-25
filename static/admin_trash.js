/* This file is licensed under AGPL-3.0 */

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
