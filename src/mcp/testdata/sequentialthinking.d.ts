declare const sequentialthinking: {
  /** A detailed tool for dynamic and reflective problem-solving through thoughts. This tool helps analyze problems through a flexible thinking process that can adapt and evolve. Each thought can build on, question, or revise previous insights as understanding deepens. When to use this tool: - Breaking down complex problems into steps - Planning and design with room for revision - Analysis that might need course correction - Problems where the full scope might not be clear initially - Problems that require a multi-step solution - Tasks that need to maintain context over multiple steps - Situations where irrelevant information needs to be filtered out Key features: - You can adjust total_thoughts up or down as you progress - You can question or revise previous thoughts - You can add more thoughts even after reaching what seemed like the end - You can express uncertainty and explore alternative approaches - Not every thought needs to build linearly - you can branch or backtrack - Generates a solution hypothesis - Verifies the hypothesis based on the Chain of Thought steps - Repeats the process until satisfied - Provides a correct answer Parameters explained: - thought: Your current thinking step, which can include: * Regular analytical steps * Revisions of previous thoughts * Questions about previous decisions * Realizations about needing more analysis * Changes in approach * Hypothesis generation * Hypothesis verification - nextThoughtNeeded: True if you need more thinking, even if at what seemed like the end - thoughtNumber: Current number in sequence (can go beyond initial total if needed) - totalThoughts: Current estimate of thoughts needed (can be adjusted up/down) - isRevision: A boolean indicating if this thought revises previous thinking - revisesThought: If is_revision is true, which thought number is being reconsidered - branchFromThought: If branching, which thought number is the branching point - branchId: Identifier for the current branch (if any) - needsMoreThoughts: If reaching end but realizing more thoughts needed You should: 1. Start with an initial estimate of needed thoughts, but be ready to adjust 2. Feel free to question or revise previous thoughts 3. Don't hesitate to add more thoughts if needed, even at the "end" 4. Express uncertainty when present 5. Mark thoughts that revise previous thinking or branch into new paths 6. Ignore information that is irrelevant to the current step 7. Generate a solution hypothesis when appropriate 8. Verify the hypothesis based on the Chain of Thought steps 9. Repeat the process until satisfied with the solution 10. Provide a single, ideally correct answer as the final output 11. Only set nextThoughtNeeded to false when truly done and a satisfactory answer is reached */
  sequentialthinking(params: {
    /** Branching point thought number */
    branchFromThought?: number;
    /** Branch identifier */
    branchId?: string;
    /** Whether this revises previous thinking */
    isRevision?: boolean;
    /** If more thoughts are needed */
    needsMoreThoughts?: boolean;
    /** Whether another thought step is needed */
    nextThoughtNeeded?: boolean;
    /** Which thought is being reconsidered */
    revisesThought?: number;
    /** Your current thinking step */
    thought: string;
    /** Current thought number (numeric value, e.g., 1, 2, 3) */
    thoughtNumber: number;
    /** Estimated total thoughts needed (numeric value, e.g., 5, 10) */
    totalThoughts: number;
  }): Promise<{
    branches: string[];
    nextThoughtNeeded: boolean;
    thoughtHistoryLength: number;
    thoughtNumber: number;
    totalThoughts: number;
  }>;
};
