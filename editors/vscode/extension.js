// LiPi for VS Code: starts `lipi lsp` and connects it to the editor.
const vscode = require('vscode');
const { LanguageClient } = require('vscode-languageclient/node');

let client;

function activate(context) {
  const command = vscode.workspace.getConfiguration('lipi').get('path') || 'lipi';
  const server = { command, args: ['lsp'] };
  client = new LanguageClient(
    'lipi',
    'LiPi Language Server',
    { run: server, debug: server },
    { documentSelector: [{ scheme: 'file', language: 'lipi' }, { scheme: 'untitled', language: 'lipi' }] }
  );
  client.start().catch((err) => {
    vscode.window.showWarningMessage(
      `LiPi: couldn't start "${command} lsp" (${err.message}). Put lipi on your PATH or set "lipi.path" in settings.`
    );
  });
  context.subscriptions.push({ dispose: () => client && client.stop() });
}

function deactivate() {
  return client ? client.stop() : undefined;
}

module.exports = { activate, deactivate };
