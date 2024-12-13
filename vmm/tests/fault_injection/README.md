The fault injection tests are disabled by default.

Kernel needs to be compiled with:

```
CONFIG_DEBUG_FS=y
CONFIG_FUNCTION_ERROR_INJECTION=y
CONFIG_FAULT_INJECTION=y
CONFIG_FAULT_INJECTION_DEBUG_FS=y
CONFIG_FAIL_FUNCTION=y
```

Because these tests cause kernel to inject failures into kernel,
they are *NOT* safe to run concurrently with other tests. To run
these tests, and only these tests:

```sh
cargo test -p vmm --test fault-injection -- --ignored --test-threads 1
```
