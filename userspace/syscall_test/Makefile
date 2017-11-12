USERSPACE_BUILD_DIR ?= build

all: syscall_test.s syscall_receive.s
	@mkdir -p ../$(USERSPACE_BUILD_DIR)
# @nasm -f bin syscall_receive.s -o ../$(USERSPACE_BUILD_DIR)/syscall_receive
	
	@nasm -f elf64 syscall_test.s -o ../$(USERSPACE_BUILD_DIR)/syscall_test.o
	@ld -o ../$(USERSPACE_BUILD_DIR)/syscall_test ../$(USERSPACE_BUILD_DIR)/syscall_test.o

clean:
	@rm -rf build
