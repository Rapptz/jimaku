/* This file is licensed under AGPL-3.0 */

const STATUS_INFO = Object.freeze({
  0: {text: 'Pending', className: 'warning' },
  1: {text: 'Rejected', className: 'danger' },
  2: {text: 'Solved', className: 'success' },
});

const getReportStatusInfo = (status) => {
  return STATUS_INFO[status] ?? {text: 'Unknown' };
};

const resolveReportModal = document.getElementById('resolve-report-modal');
const confirmResolveReportButton = document.getElementById('confirm-resolve-report');

const loadMoreReports = document.getElementById('load-more-reports');
const loadingSpinner = document.getElementById('loading-report-spinner');

confirmResolveReportButton?.addEventListener('click', e => {
  e.preventDefault();
  const form = resolveReportModal?.querySelector('form');
  if(form?.reportValidity()) {
    resolveReport()
    form.reset();
  }
});

resolveReportModal?.querySelector('button[formmethod=dialog]')?.addEventListener('click', e => {
  e.preventDefault();
  resolveReportModal.close();
});

async function resolveReport() {
  const reportId = resolveReportModal.dataset.id;
  const payload = {
    status: parseInt(document.getElementById('report-status').value),
    response: document.getElementById('report-response').value,
  };

  let report = await callApi(`/report/${reportId}/`, {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
    },
    body: JSON.stringify(payload),
  });

  if(report === null) {
    return;
  }

  if(resolveReportModal.reportInfo != null) {
    resolveReportModal.reportInfo.setAttribute('status', report.status);
    resolveReportModal.reportInfo.setAttribute('response', report.response);
    delete resolveReportModal.reportInfo;
  }

  showAlert({level: 'success', content: 'Successfully resolved report. The author will be notified soon.'});
  resolveReportModal.close();
}

/**
 * Attributes:
 * - display: 'romaji' | 'native' | 'english'
 * - fallback: Fallback text to display if the entry is not found
 * - data-id: ID of the entry
 * - data-romaji: The romaji name
 * - data-native: The native name
 * - data-english: The english name
 */
class EntryLink extends HTMLElement {
  static observedAttributes = ['display'];
  constructor() {
    super();
  }

  connectedCallback() {
    const shadow = this.attachShadow({mode: 'open'});

    const display = this.getAttribute('display') ?? 'romaji';
    const id = this.getAttribute('data-id');
    const text = this.getAttribute(`data-${display}`);

    const style = document.createElement('style');
    style.textContent = `
      :host {
        font-family: var(--font-family);
        font-size: 16px;
        color: var(--foreground);
        line-height: 1.5;
      }
      a {
        color: var(--link-text);
        text-decoration: none;
      }
      a:hover {
        color: var(--link-hover-text);
        text-decoration: underline;
      }
      span {
        font-weight: bold;
      }
    `;

    shadow.appendChild(style);

    if(id != null) {
      shadow.appendChild(html('a', { href: `/entry/${id}` }, text ?? this.getAttribute('data-romaji')));
    } else {
      const fallback = this.getAttribute('fallback') ?? 'Unknown Entry';
      shadow.appendChild(html('span', fallback));
    }
  }

  attributeChangedCallback(name, oldValue, newValue) {
    if (name === 'display') {
      const shadow = this.shadowRoot;
      if (!shadow) return;

      const text = this.getAttribute(`data-${newValue}`);
      if(text && shadow.lastChild) {
        shadow.lastChild.textContent = text;
      }
    }
  }
}

/**
 * Attributes:
 * status: The report status (as a number)
 * response: The response text
 * value: The JSON payload of the RichReport object
 * hide-user: Whether to hide the user who made the report
 * hide-actions: Whether to show the response button
 */
class ReportInfo extends HTMLElement {
  static observedAttributes = ['status', 'response', 'value'];
  constructor() {
    super();

    this.attachShadow({mode: 'open'});
    this.statusPill = html('span.status');
    this.responseText = html('span.response.empty');
  }

  connectedCallback() {
    const style = document.createElement('style');
    style.textContent = `
      :host {
        font-family: var(--font-family);
        font-size: 16px;
        line-height: 1.5;
        color: var(--foreground);
      }
      a {
        color: var(--link-text);
        text-decoration: none;
      }
      a:hover {
        color: var(--link-hover-text);
        text-decoration: underline;
      }

      details {
        border: 2px solid var(--box-border);
        border-radius: 5px;
      }

      details > summary {
        list-style: none;
        display: flex;
        background-color: var(--box-shade);
        justify-content: space-between;
        align-items: center;
        padding: 0.5rem 1rem;
        user-select: none;
        cursor: pointer;
        border-radius: 5px;
      }

      details > summary::marker,
      details > summary::-webkit-details-marker {
        display: none;
      }

      details[open]:not(.empty) > summary {
        border-radius: 5px 5px 0 0;
        border-bottom: 2px solid var(--box-border);
      }

      details.empty > summary {
        cursor: default;
      }

      details > summary::after {
        content: '\\25B6';
        transition: 0.2s;
      }

      details[open] > summary::after {
        transform: rotate(90deg);
      }

      details.empty > summary::after {
        display: none;
      }

      .content, .description {
        display: flex;
        flex-direction: column;
      }

      .content {
        padding: 1rem;
        gap: 0.25rem;
      }

      .status {
        border: 1px solid var(--foreground);
        letter-spacing: 0;
        font-size: 12px;
        border-radius: 5px;
        padding: 2px 4px;
        margin-right: 0.5rem;
      }

      .status.warning {
        background-color: var(--warning-bg);
        color: var(--warning-text);
        border-color: var(--warning-border);
      }

      .status.info {
        background-color: var(--info-bg);
        color: var(--info-text);
        border-color: var(--info-border);
      }

      .status.danger {
        background-color: var(--error-bg);
        color: var(--error-text);
        border-color: var(--error-border);
      }

      .status.success {
        background-color: var(--success-bg);
        color: var(--success-text);
        border-color: var(--success-border);
      }

      .status.branding {
        border-color: var(--box-border);
        color: var(--branding);
        background: var(--box);
      }

      .reason::before {
        content: 'Reason: ';
        font-weight: bold;
      }

      .response:not(.empty)::before {
        content: 'Response: ';
        font-weight: bold;
      }

      button {
        display: flex;
        font-family: var(--font-family);
        font-size: 16px;
        min-height: 2rem;
        justify-content: center;
        align-items: center;
        color: var(--button-text);
        background-color: rgb(var(--primary-button-rgb));
        border: 1px solid rgb(var(--primary-button-rgb));
        border-radius: 0.25rem;
        font-weight: 500;
        padding: 0.125rem 1rem;
        text-decoration: none;
        -webkit-user-select: none;
        -moz-user-select: none;
        user-select: none;
        -webkit-appearance: none;
        -moz-appearance: none;
        appearance: none;
        cursor: pointer;
        width: max-content;
      }

      button:disabled {
        cursor: default;
        opacity: 0.7;
      }

      button:hover:not(:disabled) {
        opacity: 0.9;
      }
    `;
    this.shadowRoot?.appendChild(style);
  }

  updateElement(info) {
    const shadow = this.shadowRoot;
    if(!shadow) return;

    const previousNode = shadow.querySelector('details');
    if(previousNode) {
      shadow.removeChild(previousNode);
    }

    const dtFormat = new Intl.DateTimeFormat(undefined, {
      dateStyle: 'full',
      timeStyle: 'medium',
    });

    const report = info.report;
    const hideUser = this.hasAttribute('hide-user');
    const hideActions = this.hasAttribute('hide-actions') || report.status != 0;

    const statusInfo = getReportStatusInfo(report.status);
    this.statusPill.textContent = statusInfo.text;
    this.statusPill.className = `status ${statusInfo.className}`;
    this.responseText.textContent = report.response;
    let length = report.response?.length ?? 0;
    this.responseText.classList.toggle('empty', length === 0);

    const files = report.payload.files.map(f =>
      html('li',
        report.entry_id != null ?
        html('a', {href: `/entry/${report.entry_id}/download/${encodeURIComponent(f)}`}, f) :
        f,
      )
    );

    const respondButton = html('button', 'Respond');
    respondButton.addEventListener('click', (e) => {
      if(resolveReportModal != null) {
        resolveReportModal.reportInfo = this;
        resolveReportModal.dataset.id = report.id;
        resolveReportModal.showModal();
      }
    });

    const container = html('details',
        html('summary',
          html('div.description',
            html('div.reporter',
              this.statusPill,
              hideUser ? null : html('a', {href: `/user/${info.account_name}`}, info.account_name),
              hideUser ? null : ' reported ',
              html('entry-link', {
                display: localStorage.getItem('preferred-name') ?? 'romaji',
              }, info.entry != null ? {
                dataset: {
                  id: report.entry_id,
                  romaji: info.entry.name,
                  native: info.entry.japanese_name,
                  english: info.entry.english_name,
                }
              } : {
                fallback: report.payload.name,
              })
            ),
            html('span.date', formatRelative(Math.floor(report.id / 1000)), {title: dtFormat.format(new Date(report.id))})
          )
        ),
        html('div.content',
          html('span.reason', report.reason),
          this.responseText,
          files.length != 0 ? html('ul', files) : null,
          hideActions ? null : html('div.buttons', respondButton),
        )
      );

    shadow.appendChild(container);
  }

  attributeChangedCallback(name, oldValue, newValue) {
    if (name === 'status') {
      const statusInfo = getReportStatusInfo(newValue)
      this.statusPill.textContent = statusInfo.text;
      this.statusPill.className = `status ${statusInfo.className}`;
    } else if (name === 'response') {
      this.responseText.textContent = newValue;
      this.responseText.classList.toggle('empty', newValue.length === 0);
    } else if (name === 'value') {
      this.updateElement(JSON.parse(newValue));
    }
  }
}

customElements.define('report-info', ReportInfo);
customElements.define('entry-link', EntryLink);

function processData(reports) {
  const reportContainer = document.getElementById('reports-container');

  for(const report of reports) {
    const info = new ReportInfo();
    if(reportContainer.hasAttribute('data-hide-user')) {
      info.setAttribute('hide-user', 'yes');
    }
    if(reportContainer.hasAttribute('data-hide-actions')) {
      info.setAttribute('hide-actions', 'yes');
    }
    info.updateElement(report);
    reportContainer.appendChild(info);
  }

  reportContainer.classList.remove('hidden');
  loadMoreReports?.classList?.remove("hidden");
  loadingSpinner?.classList?.add("hidden");
}

async function getReports(before) {
  const container = document.getElementById('reports-container');
  if(loadMoreReports) {
    loadMoreReports.textContent = "Loading...";
    loadMoreReports.disabled = true;
  }

  let params = new URL(document.location).searchParams;
  if(before) params.append('before', before);
  if(container.dataset.userId) params.append('account_id', container.dataset.userId);

  let response = await fetch('/reports/query?' + params);
  if(response.status !== 200) {
    showAlert({level: 'error', content: `Reports failed to show: server responded with ${response.status}`});
    loadingSpinner?.classList?.add('hidden');
    return;
  }

  let reports = await response.json();
  processData(reports);
  if(reports.length !== 100) {
    if(before) {
      if(loadMoreReports) {
        loadMoreReports.disabled = true;
        loadMoreReports.textContent = 'No more reports';
      }
    } else {
      loadMoreReports?.classList?.add('hidden');
      if(reports.length === 0) {
        container.appendChild(html('p', 'No reports!'));
      }
    }
  } else {
    if(loadMoreReports) {
      loadMoreReports.textContent = 'Load more';
      loadMoreReports.dataset.lastId = reports[reports.length - 1].id;
      loadMoreReports.disabled = false;
    }
  }
}

settings.addEventListener('preferred-name', (value) => {
  document.querySelectorAll('entry-link').forEach((link) => {
    link.setAttribute('display', value);
  });
})

document.addEventListener('DOMContentLoaded', () => {
  if(document.getElementById('reports-container')) {
    getReports();
  }
});
loadMoreReports?.addEventListener('click', () => getReports(loadMoreReports.dataset.lastId));
