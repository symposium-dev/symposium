"use strict";
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();
Object.defineProperty(exports, "__esModule", { value: true });
const assert = __importStar(require("assert"));
const homerActor_1 = require("../actors/homerActor");
suite('HomerActor Test Suite', () => {
    test('HomerActor yields quote chunks', async () => {
        const actor = new homerActor_1.HomerActor();
        const chunks = [];
        // Collect all chunks from the first prompt
        for await (const chunk of actor.sendPrompt('test prompt')) {
            chunks.push(chunk);
        }
        // Should have received multiple chunks
        assert.ok(chunks.length > 0, 'Should receive at least one chunk');
        // Concatenated chunks should form a complete quote
        const fullQuote = chunks.join('');
        assert.ok(fullQuote.length > 0, 'Quote should not be empty');
        console.log(`Received ${chunks.length} chunks: "${fullQuote}"`);
    });
    test('HomerActor cycles through quotes', async () => {
        const actor = new homerActor_1.HomerActor();
        const quotes = [];
        // Get three quotes
        for (let i = 0; i < 3; i++) {
            const chunks = [];
            for await (const chunk of actor.sendPrompt(`test ${i}`)) {
                chunks.push(chunk);
            }
            quotes.push(chunks.join(''));
        }
        // Should have three different quotes (though theoretically they could repeat if array is small)
        assert.strictEqual(quotes.length, 3, 'Should receive three quotes');
        console.log('First three quotes:');
        quotes.forEach((q, i) => console.log(`  ${i + 1}. ${q.substring(0, 50)}...`));
    });
});
//# sourceMappingURL=homerActor.test.js.map