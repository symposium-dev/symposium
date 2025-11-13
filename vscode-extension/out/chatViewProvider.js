"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.ChatViewProvider = void 0;
const symposiumPanel_1 = require("./symposiumPanel");
class ChatViewProvider {
    _extensionUri;
    static viewType = "symposium.chatView";
    constructor(_extensionUri) {
        this._extensionUri = _extensionUri;
    }
    resolveWebviewView(webviewView, context, _token) {
        // Create or show the Symposium panel
        symposiumPanel_1.SymposiumPanel.createOrShow(webviewView, this._extensionUri);
    }
}
exports.ChatViewProvider = ChatViewProvider;
//# sourceMappingURL=chatViewProvider.js.map