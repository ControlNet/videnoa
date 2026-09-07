# QA Timeout Cleanup Ordering

When a timeout probe and its leak detector run concurrently, the detector can correctly observe the child while the bounded command is still active and falsely report a post-timeout leak.

Use this ordering for reliable cleanup evidence:

1. Launch the bounded command with a unique process name.
2. Wait for `timeout` to return and record exit `124`.
3. Only then check for surviving children.
4. Repeat the check after cleanup to prove idempotence.

For diagnosis, observing the unique child during the timeout and proving it is absent after `wait` distinguishes a harness race from a real process leak.
