# Input admission failure and relay timeout diagnosis

- Reported NAS logs show an `input_changed` upload-stage failure followed about 34 seconds later by an iroh relay `Ping timeout`. A second task was reserved and entered uploading between these events.
- `scheduler/upload_input.rs::open_verified` compares persisted input size, platform identity, optional content hash, and millisecond mtime; any mismatch returns `InputChanged`. Errors in the final retained-file/path revalidation also map to that code.
- `scheduler/upload_fresh.rs` performs this check before calling the remote upload client. An `uploading` lifecycle label therefore does not prove that input bytes were sent.
- The code reports no mismatching field in this log. Source replacement/modification and filesystem identity changes are hypotheses to investigate, not established causes. Collect task input path, admission/deployment timing, mount configuration, and evidence of any file writers before choosing a fix.
- The later relay warning means an established relay connection lost its ping response deadline. It has no task correlation in the supplied excerpt; it does not establish the cause of the earlier local admission failure or the outcome of the second task.
- No source fix was made from this excerpt alone. Once the intended source is stable, a newly created task captures its current input snapshot. Do not bypass input integrity checks or modify persisted identity values to force an old task through.
