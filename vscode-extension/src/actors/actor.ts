/**
 * Actor interface for handling user prompts and generating responses.
 * Implementations can be dummy actors (like Homer) or real ACP clients.
 */
export interface Actor {
  /**
   * Send a prompt to the actor and receive a streaming response.
   * @param prompt The user's prompt text
   * @returns AsyncGenerator yielding response chunks
   */
  sendPrompt(prompt: string): AsyncGenerator<string>;
}
