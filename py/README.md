# Serial Keel python side

It's a work in progress.
But the summary is:

- Start the server in one terminal: `cargo r`
- Run pytest: `pytest ./py -o log_cli=true --log-cli-level=INFO`

That's it.
The mocked session is the output from flashing and capturing the output of `./py/zephyr.hex`.

This hex file is from [the nRF Connect SDK](https://github.com/nrfconnect/sdk-nrf/tree/main/tests/crypto), and it's specifically a crypto test suite for the Nrf5340 chip using the Arm CryptoCell accelerator.
