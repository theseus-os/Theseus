all: syscall_test.s syscall_receive.s
	@mkdir -p ../$(USERSPACE_BUILD_DIR)
	@nasm -f bin syscall_test.s -o ../$(USERSPACE_BUILD_DIR)/syscall_test
	@nasm -f bin syscall_receive.s -o ../$(USERSPACE_BUILD_DIR)/syscall_receive
