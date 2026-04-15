## Included Examples

* Running in an unikernel micro-VM using [Unikraft](https://unikraft.org/):
  ```sh
  kraft run -M 256M -p 3569:3569
  ```
  * Note for MacOS users: Best run the micro-VM via Qemu network backend "vmnet", which was added by the developer of AxleOS.
* Running containerized in Docker:
  ```sh
  docker build -t flowd-rs:local .
  docker run --rm -p 3569:3569 flowd-rs:local /flowd-rs 0.0.0.0:3569
  ```

For more see [README_WIP](../README_WIP.md).