# JSON-RPC Test Suite

This document tracks the test coverage for the JSON-RPC layer.

## Basic Communication

- [x] **Request/Response round-trip** - Send request, receive response (ping/pong test)
- [x] **Notification (one-way message)** - Send a notification that doesn't expect a response
- [x] **Multiple sequential requests** - Send several requests in order and verify responses
- [x] **Concurrent requests** - Send multiple requests at once and verify all responses arrive

## Handler Chain Behavior

- [x] **Multiple handlers with different methods** - Handler1 handles "foo", Handler2 handles "bar"
- [x] **Handler priority/ordering** - First handler in chain gets first chance to claim
- [x] **Fallthrough behavior** - Request passes through Handler1 (doesn't claim) to Handler2 (claims)
- [x] **No handler claims** - What happens when no handler claims a request?
- [x] **Handler can claim notifications** - Test notification handling in chain

## Error Handling

- [ ] **Invalid JSON** - Send malformed JSON and verify error response (IGNORED - hangs, needs investigation)
- [x] **Unknown method** - Send request with method no handler claims
- [x] **Handler returns error** - Handler explicitly returns an error
- [ ] **Serialization errors** - Response that can't be serialized (TODO)
- [ ] **Request without required params** - Missing or invalid parameters (IGNORED - hangs, needs investigation)

## Edge Cases

- [ ] **Empty request** - Request with no parameters
- [ ] **Null parameters** - Request with explicit null params
- [ ] **Server shutdown** - What happens to pending requests when server stops?
- [ ] **Client disconnect** - What happens when client disconnects mid-request?

## Advanced Features

- [ ] **Bidirectional communication** - Both sides can be server+client simultaneously
- [ ] **Request IDs** - Verify responses match request IDs correctly
- [ ] **Out-of-order responses** - Responses can arrive in different order than requests

## Notes

- Skipping stress tests (large payloads, rapid fire requests) for now
- Focus on correctness and edge cases rather than performance
