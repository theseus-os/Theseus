all: test_program.s
	@mkdir -p ../$(USERSPACE_BUILD_DIR)

	@nasm -f elf64 test_program.s -o ../$(USERSPACE_BUILD_DIR)/test_program.o
	@ld -o ../$(USERSPACE_BUILD_DIR)/test_program ../$(USERSPACE_BUILD_DIR)/test_program.o
	@strip ../$(USERSPACE_BUILD_DIR)/test_program

clean:
	@rm -rf build
