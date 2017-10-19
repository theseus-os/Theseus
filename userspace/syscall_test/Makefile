all: syscall_test.s syscall_receive.s
	@mkdir -p ../$(USERSPACE_BUILD_DIR)
	@nasm -f bin syscall_test.s -o ../$(USERSPACE_BUILD_DIR)/syscall_test


clean:
	@rm -rf build
