const path = require("path");
const vscode = require("vscode");
const lc = require("vscode-languageclient/node");

function activate(context) {
    const serverOptions = {
        command: path.join(__dirname, "haxlint")
    };
    console.log("haxlint path:", serverOptions.command);

    const clientOptions = {
        documentSelector: [{ scheme: "file", language: "rust" }]
    };

    const client = new lc.LanguageClient(
        "haxlint",
        "Hax Linter",
        serverOptions,
        clientOptions
    );

    context.subscriptions.push(client.start());
    console.log("running")
}

exports.activate = activate;
exports.deactivate = () => {};