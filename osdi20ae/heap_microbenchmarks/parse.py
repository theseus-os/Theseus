from prettytable import PrettyTable

# threadtest mean, threadtest std dev, shbench mean, shbench std dev
unsafe = [0,0,0,0]
safe = [0,0,0,0]
partially_safe = [0,0,0,0]

for line in open('./unsafe/results.txt','r'):
    if 'HEAP_EVAL:' in line:
        tokens = line.split()
        if 'threadtest' in tokens:
            unsafe[0] = float(tokens[4])/1000000
            unsafe[1]= float(tokens[5])/1000000
        if 'shbench' in tokens:
            unsafe[2] = float(tokens[4])/1000000
            unsafe[3]= float(tokens[5])/1000000

for line in open('./safe/results.txt','r'):
    if 'HEAP_EVAL:' in line:
        tokens = line.split()
        if 'threadtest' in tokens:
            safe[0] = float(tokens[4])/1000000
            safe[1]= float(tokens[5])/1000000
        if 'shbench' in tokens:
            safe[2] = float(tokens[4])/1000000
            safe[3]= float(tokens[5])/1000000

for line in open('./partially_safe/results.txt','r'):
    if 'HEAP_EVAL:' in line:
        tokens = line.split()
        if 'threadtest' in tokens:
            partially_safe[0] = float(tokens[4])/1000000
            partially_safe[1]= float(tokens[5])/1000000
        if 'shbench' in tokens:
            partially_safe[2] = float(tokens[4])/1000000
            partially_safe[3]= float(tokens[5])/1000000


print("Heap microbenchmark results, corresponding to Table 2. All results are in seconds.")
print()
table = PrettyTable()
table.field_names = ["Heap Design", "threadtest mean", "threadtest std dev ", "shbench mean", "shbench std dev"]
table.add_row(["unsafe", unsafe[0], unsafe[1], unsafe[2], unsafe[3]])
table.add_row(["partially-safe", partially_safe[0], partially_safe[1], partially_safe[2], partially_safe[3]])
table.add_row(["safe", safe[0], safe[1], safe[2], safe[3]])


print(table)

