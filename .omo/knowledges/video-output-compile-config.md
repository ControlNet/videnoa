# VideoOutput Compile Configuration

- `compile_graph_with_debug_hook` resolves sink inputs before `VideoOutputNode::execute`.
- `VideoOutputNode::execute` intentionally exposes only `output_path`; encoder options are input parameters, not generic output ports.
- `CompileContext::create_encoder` receives both resolved sink inputs and execution outputs.
- `VideoCompileContext` reads `codec`, `crf`, and `pixel_format` from resolved inputs, while retaining `output_path` from outputs and dimensions/FPS from compile-context state.
- Regression coverage: `compile::tests::test_compile_preserves_custom_sink_encoder_settings` uses non-default `libx264`, CRF `27`, and `yuv444p` values.
