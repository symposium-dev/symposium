"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.HomerActor = void 0;
/**
 * Homer Actor - cycles through quotes from the Iliad and Odyssey.
 * Used as a dummy actor for testing the UI before ACP integration.
 */
class HomerActor {
    quoteIndex = 0;
    // Quotes from the Iliad and Odyssey
    quotes = [
        "Sing, O goddess, the anger of Achilles son of Peleus, that brought countless ills upon the Achaeans.",
        "Tell me, O muse, of that ingenious hero who travelled far and wide after he had sacked the famous town of Troy.",
        "There is nothing more admirable than when two people who see eye to eye keep house as man and wife, confounding their enemies and delighting their friends.",
        "Even his griefs are a joy long after to one that remembers all that he wrought and endured.",
        "For rarely are sons similar to their fathers: most are worse, and a few are better than their fathers.",
        "Hateful to me as the gates of Hades is that man who hides one thing in his heart and speaks another.",
        "The blade itself incites to deeds of violence.",
        "Light is the task where many share the toil.",
        "A sympathetic friend can be quite as dear as a brother.",
        "It is not strength, but art, obtains the prize.",
        "The difficulty is not so great to die for a friend, as to find a friend worth dying for.",
        "Words empty as the wind are best left unsaid.",
        "There is a time for many words, and there is also a time for sleep.",
        "Be strong, saith my heart; I am a soldier; I have seen worse sights than this.",
        "No man or woman born, coward or brave, can shun his destiny.",
        "It is entirely seemly for a young man killed in battle to lie mangled by the bronze spear. In his death all things appear fair.",
        "Even were I to go down into the house of Hades I should not be without a name.",
        "Two friends, two bodies with one soul inspired.",
        "Let me not then die ingloriously and without a struggle, but let me first do some great thing that shall be told among men hereafter.",
        "We mortals hear only the news, and know nothing at all.",
    ];
    async *sendPrompt(prompt) {
        // Get the next quote (cycle through the array)
        const quote = this.quotes[this.quoteIndex];
        this.quoteIndex = (this.quoteIndex + 1) % this.quotes.length;
        // Split quote into words for streaming effect
        const words = quote.split(" ");
        for (const word of words) {
            // Yield word with space (except for last word)
            const chunk = word === words[words.length - 1] ? word : word + " ";
            yield chunk;
            // Simulate network delay for streaming effect
            await new Promise((resolve) => setTimeout(resolve, 50));
        }
    }
}
exports.HomerActor = HomerActor;
//# sourceMappingURL=homerActor.js.map