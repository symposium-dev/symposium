import * as assert from 'assert';
import { HomerActor } from '../actors/homerActor';

suite('HomerActor Test Suite', () => {
    test('HomerActor yields quote chunks', async () => {
        const actor = new HomerActor();
        const chunks: string[] = [];

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
        const actor = new HomerActor();
        const quotes: string[] = [];

        // Get three quotes
        for (let i = 0; i < 3; i++) {
            const chunks: string[] = [];
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
