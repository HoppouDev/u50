/*
 * Copyright (C) 2007 The Guava Authors
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 * http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

package com.google.common.collect;
import static com.google.common.base.Preconditions.checkArgument;
import static com.google.common.base.Preconditions.checkElementIndex;
import static com.google.common.base.Preconditions.checkNotNull;
import static com.google.common.base.Preconditions.checkPositionIndexes;
import static com.google.common.collect.CollectPreconditions.checkNonnegative;
import static com.google.common.collect.Lists.equalsImpl;
import static com.google.common.collect.Lists.indexOfImpl;
import static com.google.common.collect.Lists.lastIndexOfImpl;
import static com.google.common.collect.ObjectArrays.checkElementsNotNull;
import static com.google.common.collect.RegularImmutableList.EMPTY;
import static java.lang.System.arraycopy;
import static java.util.Objects.requireNonNull;

import com.google.common.annotations.GwtCompatible;
import com.google.common.annotations.GwtIncompatible;
import com.google.common.annotations.J2ktIncompatible;
import com.google.common.annotations.VisibleForTesting;
import com.google.errorprone.annotations.CanIgnoreReturnValue;
import com.google.errorprone.annotations.DoNotCall;
import com.google.errorprone.annotations.InlineMe;
import java.io.InvalidObjectException;
import java.io.ObjectInputStream;
import java.io.Serializable;
import java.util.Arrays;
import java.util.Collection;
import java.util.Collections;
import java.util.Comparator;
import java.util.Iterator;
import java.util.List;
import java.util.RandomAccess;
import java.util.Spliterator;
import java.util.function.Consumer;
import java.util.function.UnaryOperator;
import java.util.stream.Collector;
import org.jspecify.annotations.Nullable;
@GwtCompatible
@SuppressWarnings({
    "serial",
    "TooManyParameters",
})
public abstract class ImmutableList<E>
    extends ImmutableCollection<E> implements List<E>, RandomAccess {
    public static <E> Collector<E, ?, ImmutableList<E>> toImmutableList()
    {
        return CollectCollectors.toImmutableList();
    }
    @SuppressWarnings("unchecked") public static <E> ImmutableList<E> of()
    {
        return (ImmutableList<E>) EMPTY;
    }
    public static <E> ImmutableList<E> of(E e1)
    {
        return new SingletonImmutableList<>(e1);
    }
    public static <E> ImmutableList<E> of(E e1, E e2)
    {
        return construct(e1, e2);
    }
    public static <E> ImmutableList<E> of(E e1, E e2, E e3)
    {
        return construct(e1, e2, e3);
    }
    public static <E> ImmutableList<E> of(E e1, E e2, E e3, E e4)
    {
        return construct(e1, e2, e3, e4);
    }
    public static <E> ImmutableList<E> of(E e1, E e2, E e3, E e4, E e5)
    {
        return construct(e1, e2, e3, e4, e5);
    }
    public static <E> ImmutableList<E> of(E e1, E e2, E e3, E e4, E e5, E e6)
    {
        return construct(e1, e2, e3, e4, e5, e6);
    }
    public static <E> ImmutableList<E> of(E e1, E e2, E e3, E e4, E e5, E e6, E e7)
    {
        return construct(e1, e2, e3, e4, e5, e6, e7);
    }
    public static <E> ImmutableList<E> of(E e1, E e2, E e3, E e4, E e5, E e6, E e7, E e8)
    {
        return construct(e1, e2, e3, e4, e5, e6, e7, e8);
    }
    public static <E> ImmutableList<E> of(E e1, E e2, E e3, E e4, E e5, E e6, E e7, E e8, E e9)
    {
        return construct(e1, e2, e3, e4, e5, e6, e7, e8, e9);
    }
    public static <E> ImmutableList<E> of(E e1, E e2, E e3, E e4, E e5, E e6, E e7, E e8, E e9,
                                          E e10)
    {
        return construct(e1, e2, e3, e4, e5, e6, e7, e8, e9, e10);
    }
    public static <E> ImmutableList<E> of(E e1, E e2, E e3, E e4, E e5, E e6, E e7, E e8, E e9,
                                          E e10, E e11)
    {
        return construct(e1, e2, e3, e4, e5, e6, e7, e8, e9, e10, e11);
    }
    @SafeVarargs
    public static <E> ImmutableList<E> of(E e1, E e2, E e3, E e4, E e5, E e6, E e7, E e8, E e9,
                                          E e10, E e11, E e12, E... others)
    {
        checkArgument(others.length <= Integer.MAX_VALUE - 12,
                      "the total number of elements must fit in an int");
        Object[] array = new Object[12 + others.length];
        array[0] = e1;
        array[1] = e2;
        array[2] = e3;
        array[3] = e4;
        array[4] = e5;
        array[5] = e6;
        array[6] = e7;
        array[7] = e8;
        array[8] = e9;
        array[9] = e10;
        array[10] = e11;
        array[11] = e12;
        arraycopy(others, 0, array, 12, others.length);
        return construct(array);
    }
    public static <E> ImmutableList<E> copyOf(Iterable<? extends E> elements)
    {
        checkNotNull(elements);
        return (elements instanceof Collection)
            ? copyOf((Collection < ? extends E >) elements) : copyOf(elements.iterator());
    }
    public static <E> ImmutableList<E> copyOf(Collection<? extends E> elements)
    {
        if (elements instanceof ImmutableCollection)
        {
            @SuppressWarnings("unchecked")
            ImmutableList<E> list = ((ImmutableCollection<E>) elements).asList();
            return list.isPartialView() ? asImmutableList(list.toArray()) : list;
        }
        return construct(elements.toArray());
    }
    public static <E> ImmutableList<E> copyOf(Iterator<? extends E> elements)
    {
        if (!elements.hasNext())
        {
            return of();
        }
        E first = elements.next();
        if (!elements.hasNext())
        {
            return of(first);
        }
        else
        {
            return new ImmutableList.Builder<E>().add(first).addAll(elements).build();
        }
    }
    public static <E> ImmutableList<E> copyOf(E[] elements)
    {
        switch (elements.length)
        {
            case 0:
                return of();
            case 1:
                return of(elements[0]);
            default:
                return construct(elements.clone());
        }
    }
    public static <E extends Comparable<? super E>> ImmutableList<E>
    sortedCopyOf(Iterable<? extends E> elements)
    {
        Comparable<?>[] array = Iterables.toArray(elements, new Comparable<?>[0]);
        checkElementsNotNull((Object[]) array);
        Arrays.sort(array);
        return asImmutableList(array);
    }
    public static <E> ImmutableList<E> sortedCopyOf(Comparator<? super E> comparator,
                                                    Iterable<? extends E> elements)
    {
        checkNotNull(comparator);
        @SuppressWarnings("unchecked") E[] array = (E[]) Iterables.toArray(elements);
        checkElementsNotNull(array);
        Arrays.sort(array, comparator);
        return asImmutableList(array);
    }
    private static <E> ImmutableList<E> construct(Object... elements)
    {
        return asImmutableList(checkElementsNotNull(elements));
    }
    static <E> ImmutableList<E> asImmutableList(Object[] elements)
    {
        return asImmutableList(elements, elements.length);
    }
    static <E> ImmutableList<E> asImmutableList(@Nullable Object[] elements, int length)
    {
        switch (length)
        {
            case 0:
                return of();
            case 1:
                @SuppressWarnings("unchecked") E onlyElement = (E) requireNonNull(elements[0]);
                return of(onlyElement);
            default:
                @SuppressWarnings("nullness")
                Object[] elementsWithoutTrailingNulls =
                    length < elements.length ? Arrays.copyOf(elements, length) : elements;
                return new RegularImmutableList<>(elementsWithoutTrailingNulls);
        }
    }
    ImmutableList() {}
    @Override public UnmodifiableIterator<E> iterator()
    {
        return listIterator();
    }
    @Override public final UnmodifiableListIterator<E> listIterator()
    {
        return listIterator(0);
    }
    @Override public UnmodifiableListIterator<E> listIterator(int index)
    {
        return new AbstractIndexedListIterator<E>(size(), index) {
            @Override E get(int index)
            {
                return ImmutableList.this.get(index);
            }
        };
    }
    @Override public void forEach(Consumer<? super E> consumer)
    {
        checkNotNull(consumer);
        int n = size();
        for (int i = 0; i < n; i++)
        {
            consumer.accept(get(i));
        }
    }
    @Override public int indexOf(@Nullable Object object)
    {
        return (object == null) ? -1 : indexOfImpl(this, object);
    }
    @Override public int lastIndexOf(@Nullable Object object)
    {
        return (object == null) ? -1 : lastIndexOfImpl(this, object);
    }
    @Override public boolean contains(@Nullable Object object)
    {
        return indexOf(object) >= 0;
    }
    @Override public ImmutableList<E> subList(int fromIndex, int toIndex)
    {
        checkPositionIndexes(fromIndex, toIndex, size());
        int length = toIndex - fromIndex;
        if (length == size())
        {
            return this;
        }
        else if (length == 0)
        {
            return of();
        }
        else if (length == 1)
        {
            return of(get(fromIndex));
        }
        else
        {
            return subListUnchecked(fromIndex, toIndex);
        }
    }
    ImmutableList<E> subListUnchecked(int fromIndex, int toIndex)
    {
        return new SubList(fromIndex, toIndex - fromIndex);
    }
    private final class SubList extends ImmutableList<E> {
        final transient int offset;
        final transient int length;
        SubList(int offset, int length)
        {
            this.offset = offset;
            this.length = length;
        }
        @Override public int size()
        {
            return length;
        }
        @Override public E get(int index)
        {
            checkElementIndex(index, length);
            return ImmutableList.this.get(index + offset);
        }
        @Override public ImmutableList<E> subList(int fromIndex, int toIndex)
        {
            checkPositionIndexes(fromIndex, toIndex, length);
            return ImmutableList.this.subList(fromIndex + offset, toIndex + offset);
        }
        @Override boolean isPartialView()
        {
            return true;
        }
        @SuppressWarnings("RedundantOverride")
        @Override
        @J2ktIncompatible
        @GwtIncompatible
        Object writeReplace()
        {
            return super.writeReplace();
        }
    }
    @CanIgnoreReturnValue
    @Deprecated
    @Override
    @DoNotCall("Always throws UnsupportedOperationException")
    public final boolean addAll(int index, Collection<? extends E> newElements)
    {
        throw new UnsupportedOperationException();
    }
    @CanIgnoreReturnValue
    @Deprecated
    @Override
    @DoNotCall("Always throws UnsupportedOperationException")
    public final E set(int index, E element)
    {
        throw new UnsupportedOperationException();
    }
    @Deprecated
    @Override
    @DoNotCall("Always throws UnsupportedOperationException")
    public final void add(int index, E element)
    {
        throw new UnsupportedOperationException();
    }
    @CanIgnoreReturnValue
    @Deprecated
    @Override
    @DoNotCall("Always throws UnsupportedOperationException")
    public final E remove(int index)
    {
        throw new UnsupportedOperationException();
    }
    @Deprecated
    @Override
    @DoNotCall("Always throws UnsupportedOperationException")
    public final void replaceAll(UnaryOperator<E> operator)
    {
        throw new UnsupportedOperationException();
    }
    @Deprecated
    @Override
    @DoNotCall("Always throws UnsupportedOperationException")
    public final void sort(@Nullable Comparator<? super E> c)
    {
        throw new UnsupportedOperationException();
    }
    @InlineMe(replacement = "this") @Deprecated @Override public final ImmutableList<E> asList()
    {
        return this;
    }
    @Override @GwtIncompatible public Spliterator<E> spliterator()
    {
        return spliteratorWithCharacteristics(SPLITERATOR_CHARACTERISTICS);
    }
    @GwtIncompatible final Spliterator<E> spliteratorWithCharacteristics(int characteristics)
    {
        return CollectSpliterators.indexed(size(), characteristics, this::get);
    }
    @Override int copyIntoArray(@Nullable Object[] dst, int offset)
    {
        int size = size();
        for (int i = 0; i < size; i++)
        {
            dst[offset + i] = get(i);
        }
        return offset + size;
    }
    public ImmutableList<E> reverse()
    {
        return (size() <= 1) ? this : new ReverseImmutableList<E>(this);
    }
    private static final class ReverseImmutableList<E> extends ImmutableList<E> {
        private final transient ImmutableList<E> forwardList;
        ReverseImmutableList(ImmutableList<E> backingList)
        {
            this.forwardList = backingList;
        }
        private int reverseIndex(int index)
        {
            return (size() - 1) - index;
        }
        private int reversePosition(int index)
        {
            return size() - index;
        }
        @Override public ImmutableList<E> reverse()
        {
            return forwardList;
        }
        @Override public boolean contains(@Nullable Object object)
        {
            return forwardList.contains(object);
        }
        @Override public int indexOf(@Nullable Object object)
        {
            int index = forwardList.lastIndexOf(object);
            return (index >= 0) ? reverseIndex(index) : -1;
        }
        @Override public int lastIndexOf(@Nullable Object object)
        {
            int index = forwardList.indexOf(object);
            return (index >= 0) ? reverseIndex(index) : -1;
        }
        @Override public ImmutableList<E> subList(int fromIndex, int toIndex)
        {
            checkPositionIndexes(fromIndex, toIndex, size());
            return forwardList.subList(reversePosition(toIndex), reversePosition(fromIndex))
                .reverse();
        }
        @Override public E get(int index)
        {
            checkElementIndex(index, size());
            return forwardList.get(reverseIndex(index));
        }
        @Override public int size()
        {
            return forwardList.size();
        }
        @Override boolean isPartialView()
        {
            return forwardList.isPartialView();
        }
        @SuppressWarnings("RedundantOverride")
        @Override
        @J2ktIncompatible
        @GwtIncompatible
        Object writeReplace()
        {
            return super.writeReplace();
        }
    }
    @Override public final boolean equals(@Nullable Object obj)
    {
        return equalsImpl(this, obj);
    }
    @Override public final int hashCode()
    {
        int hashCode = 1;
        int n = size();
        for (int i = 0; i < n; i++)
        {
            hashCode = 31 * hashCode + get(i).hashCode();
            hashCode = ~~hashCode;
        }
        return hashCode;
    }
    @J2ktIncompatible
    static final class SerializedForm implements Serializable {
        final Object[] elements;
        SerializedForm(Object[] elements)
        {
            this.elements = elements;
        }
        Object readResolve()
        {
            return copyOf(elements);
        }
        @GwtIncompatible private static final long serialVersionUID = 0;
    }
    @J2ktIncompatible
    private void readObject(ObjectInputStream stream) throws InvalidObjectException
    {
        throw new InvalidObjectException("Use SerializedForm");
    }
    @Override @J2ktIncompatible @GwtIncompatible Object writeReplace()
    {
        return new SerializedForm(toArray());
    }
    public static <E> Builder<E> builder()
    {
        return new Builder<>();
    }
    public static <E> Builder<E> builderWithExpectedSize(int expectedSize)
    {
        checkNonnegative(expectedSize, "expectedSize");
        return new ImmutableList.Builder<>(expectedSize);
    }
    public static final class Builder<E> extends ImmutableCollection.Builder<E> {
        @VisibleForTesting @Nullable Object[] contents;
        private int size;
        private boolean copyOnWrite;
        public Builder()
        {
            this(DEFAULT_INITIAL_CAPACITY);
        }
        Builder(int capacity)
        {
            this.contents = new @Nullable Object[capacity];
            this.size = 0;
        }
        private void ensureRoomFor(int newElements)
        {
            @Nullable Object[] contents = this.contents;
            int newCapacity = expandedCapacity(contents.length, size + newElements);
            if (contents.length < newCapacity || copyOnWrite)
            {
                this.contents = Arrays.copyOf(contents, newCapacity);
                copyOnWrite = false;
            }
        }
        @CanIgnoreReturnValue @Override public Builder<E> add(E element)
        {
            checkNotNull(element);
            ensureRoomFor(1);
            contents[size++] = element;
            return this;
        }
        @CanIgnoreReturnValue @Override public Builder<E> add(E... elements)
        {
            checkElementsNotNull(elements);
            add(elements, elements.length);
            return this;
        }
        private void add(@Nullable Object[] elements, int n)
        {
            ensureRoomFor(n);
            arraycopy(elements, 0, contents, size, n);
            size += n;
        }
        @CanIgnoreReturnValue @Override public Builder<E> addAll(Iterable<? extends E> elements)
        {
            checkNotNull(elements);
            if (elements instanceof Collection)
            {
                Collection<?> collection = (Collection<?>) elements;
                ensureRoomFor(collection.size());
                if (collection instanceof ImmutableCollection)
                {
                    ImmutableCollection<?> immutableCollection =
                        (ImmutableCollection<?>) collection;
                    size = immutableCollection.copyIntoArray(contents, size);
                    return this;
                }
            }
            super.addAll(elements);
            return this;
        }
        @CanIgnoreReturnValue @Override public Builder<E> addAll(Iterator<? extends E> elements)
        {
            super.addAll(elements);
            return this;
        }
        @CanIgnoreReturnValue Builder<E> combine(Builder<E> builder)
        {
            checkNotNull(builder);
            add(builder.contents, builder.size);
            return this;
        }
        @Override public ImmutableList<E> build()
        {
            copyOnWrite = true;
            return asImmutableList(contents, size);
        }
        @SuppressWarnings("unchecked")
        ImmutableList<E> buildSorted(Comparator<? super E> comparator)
        {
            copyOnWrite = true;
            Arrays.sort((E[]) contents, 0, size, comparator);
            return asImmutableList(contents, size);
        }
    }
    @GwtIncompatible @J2ktIncompatible private static final long serialVersionUID = 0xcafebabe;
}
