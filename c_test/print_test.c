// Basic test printf functions

#include <stdio.h>
#include <string.h>

// create some named .rodata
const char *const HELLO = "hello world";

// create some named .data and unnamed .rodata
char *s = "me";


int main(int argc, char *argv[]) {
	printf("Printing 17: %d\n", 17);
	for (int i = 0; i < argc; i++) {
		printf("arg %u: %s\n", i, argv[i]);
	}

	printf("HELLO STRING: %s (len %ld)\n", HELLO, strlen(HELLO));

	return 0x1234;
}
