# Serial Keel

It's a work in progress.
But the summary is:

- Start the server:
    - `cargo r`, or `cargo r --release`
	- Precompiled: `./bin/serial-keel`

- Run `python py/test_crypto.py`
- Run `python py/test_tfm.py`

That's it.

## test_crypto_test_app

This mocked test case is the output from flashing and capturing the output of `./py/zephyr.hex`.
This hex file is from [the nRF Connect SDK](https://github.com/nrfconnect/sdk-nrf/tree/main/tests/crypto), and it's specifically a crypto test suite for the Nrf5340 chip using the Arm CryptoCell accelerator.

## test_tfm_regression

This test case if from [the nRF Connect SDK](https://github.com/nrfconnect/sdk-zephyr/tree/main/samples/tfm_integration/tfm_regression_test).
The hex file is not added to the repo, TODO.
